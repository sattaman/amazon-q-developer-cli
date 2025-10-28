use crate::observability::events::TraceEvent;
use serde::Serialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

const LANGFUSE_API_URL: &str = "https://cloud.langfuse.com";
const BATCH_SIZE: usize = 2;  // Small batch for testing
const FLUSH_INTERVAL_SECS: u64 = 5;  // Reasonable timer
const MAX_RETRIES: u32 = 3;

#[derive(Debug, Serialize)]
struct LangfuseEvent {
    id: String,
    timestamp: String,
    #[serde(rename = "type")]
    event_type: String,
    body: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct LangfuseBatch {
    batch: Vec<LangfuseEvent>,
}

pub struct LangfuseSink {
    api_key: String,
    public_key: String,
    _api_url: String,
    _client: reqwest::Client,
    tx: mpsc::UnboundedSender<TraceEvent>,
    flush_tx: mpsc::UnboundedSender<oneshot::Sender<()>>,
    success_count: Arc<AtomicUsize>,
    error_count: Arc<AtomicUsize>,
}

impl LangfuseSink {
    pub fn new(api_key: String, public_key: String, api_url: Option<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        let (tx, rx) = mpsc::unbounded_channel();
        let (flush_tx, flush_rx) = mpsc::unbounded_channel();
        
        let api_url_final = api_url.unwrap_or_else(|| LANGFUSE_API_URL.to_string());
        
        let success_count = Arc::new(AtomicUsize::new(0));
        let error_count = Arc::new(AtomicUsize::new(0));
        
        let sink = Self {
            api_key: api_key.clone(),
            public_key: public_key.clone(),
            _api_url: api_url_final.clone(),
            _client: client.clone(),
            tx,
            flush_tx,
            success_count: success_count.clone(),
            error_count: error_count.clone(),
        };

        tokio::spawn(Self::batch_worker(
            rx,
            flush_rx,
            client,
            sink.api_key.clone(),
            sink.public_key.clone(),
            api_url_final,
            success_count,
            error_count,
        ));

        Ok(sink)
    }

    pub async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        if self.flush_tx.send(tx).is_ok() {
            let _ = rx.await;
            // Give HTTP request extra time to complete before runtime shutdown
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn emit(&self, event: TraceEvent) -> Result<(), Box<dyn std::error::Error>> {
        self.tx.send(event)?;
        Ok(())
    }

    async fn batch_worker(
        mut rx: mpsc::UnboundedReceiver<TraceEvent>,
        mut flush_rx: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
        client: reqwest::Client,
        api_key: String,
        public_key: String,
        api_url: String,
        success_count: Arc<AtomicUsize>,
        error_count: Arc<AtomicUsize>,
    ) {
        let mut batch = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_secs(FLUSH_INTERVAL_SECS));

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    if let Some(lf_event) = Self::map_event(event) {
                        batch.push(lf_event);
                        if batch.len() >= BATCH_SIZE {
                            Self::flush_batch(&client, &api_key, &public_key, &api_url, &mut batch, &success_count, &error_count).await;
                        }
                    }
                }
                Some(ack) = flush_rx.recv() => {
                    // Explicit flush requested
                    if !batch.is_empty() {
                        Self::flush_batch(&client, &api_key, &public_key, &api_url, &mut batch, &success_count, &error_count).await;
                    }
                    // Send acknowledgment that flush is complete
                    let _ = ack.send(());
                }
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        Self::flush_batch(&client, &api_key, &public_key, &api_url, &mut batch, &success_count, &error_count).await;
                    }
                }
            }
        }
    }

    async fn flush_batch(
        client: &reqwest::Client,
        api_key: &str,
        public_key: &str,
        api_url: &str,
        batch: &mut Vec<LangfuseEvent>,
        success_count: &Arc<AtomicUsize>,
        error_count: &Arc<AtomicUsize>,
    ) {
        if batch.is_empty() {
            return;
        }

        let batch_len = batch.len();
        let payload = LangfuseBatch {
            batch: batch.drain(..).collect(),
        };

        for attempt in 1..=MAX_RETRIES {
            match client
                .post(format!("{}/api/public/ingestion", api_url))
                .basic_auth(public_key, Some(api_key))
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() || resp.status() == 207 => {
                    success_count.fetch_add(batch_len, Ordering::Relaxed);
                    eprintln!("✅ Langfuse: {} events sent successfully", batch_len);
                    tracing::debug!("Flushed {} events to Langfuse", batch_len);
                    return;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    error_count.fetch_add(batch_len, Ordering::Relaxed);
                    tracing::warn!("Langfuse API error (attempt {}): {} - {}", attempt, status, body);
                }
                Err(e) => {
                    error_count.fetch_add(batch_len, Ordering::Relaxed);
                    tracing::warn!("Langfuse request failed (attempt {}): {}", attempt, e);
                }
            }

            if attempt < MAX_RETRIES {
                tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
            }
        }

        tracing::error!("Failed to flush batch to Langfuse after {} retries", MAX_RETRIES);
    }

    fn map_event(event: TraceEvent) -> Option<LangfuseEvent> {
        let envelope_id = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Utc::now().to_rfc3339();
        
        match event {
            TraceEvent::UserPrompt { trace_id, turn_index, timestamp_utc, user_input } => {
                if turn_index == 0 {
                    // Create trace with user input
                    Some(LangfuseEvent {
                        id: envelope_id,
                        timestamp: timestamp.clone(),
                        event_type: "trace-create".to_string(),
                        body: serde_json::json!({
                            "id": trace_id.to_string(),
                            "name": "q-chat-session",
                            "timestamp": timestamp_utc,
                            "input": user_input,
                        }),
                    })
                } else {
                    // User prompt as span for multi-turn
                    Some(LangfuseEvent {
                        id: envelope_id,
                        timestamp: timestamp.clone(),
                        event_type: "span-create".to_string(),
                        body: serde_json::json!({
                            "id": format!("{}-turn-{}-user", trace_id, turn_index),
                            "traceId": trace_id.to_string(),
                            "name": "user_prompt",
                            "startTime": timestamp_utc,
                            "endTime": timestamp_utc,
                            "metadata": { "turn_index": turn_index },
                            "input": user_input,
                        }),
                    })
                }
            }
            TraceEvent::AgentThought { trace_id, turn_index, timestamp_utc, agent_thought_trace } => {
                Some(LangfuseEvent {
                    id: envelope_id,
                    timestamp: timestamp.clone(),
                    event_type: "span-create".to_string(),
                    body: serde_json::json!({
                        "id": format!("{}-turn-{}-thought", trace_id, turn_index),
                        "traceId": trace_id.to_string(),
                        "name": "agent_thought",
                        "startTime": timestamp_utc,
                        "endTime": timestamp_utc,
                        "metadata": { "turn_index": turn_index },
                        "output": { "thought": agent_thought_trace },
                        "level": "DEBUG"
                    }),
                })
            }
            TraceEvent::ToolExecute { trace_id, turn_index, timestamp_utc, tool_calls_executed } => {
                let tool_name = tool_calls_executed.first()?.name.clone();
                Some(LangfuseEvent {
                    id: envelope_id,
                    timestamp: timestamp.clone(),
                    event_type: "span-create".to_string(),
                    body: serde_json::json!({
                        "id": format!("{}-turn-{}-tool-{}", trace_id, turn_index, tool_name),
                        "traceId": trace_id.to_string(),
                        "name": format!("tool_{}", tool_name),
                        "startTime": timestamp_utc,
                        "endTime": timestamp_utc,
                        "metadata": { "turn_index": turn_index },
                        "input": tool_calls_executed,
                        "level": "DEFAULT"
                    }),
                })
            }
            TraceEvent::ToolOutput { trace_id, turn_index, timestamp_utc, tool_output, .. } => {
                Some(LangfuseEvent {
                    id: envelope_id,
                    timestamp: timestamp.clone(),
                    event_type: "span-create".to_string(),
                    body: serde_json::json!({
                        "id": format!("{}-turn-{}-tool-output", trace_id, turn_index),
                        "traceId": trace_id.to_string(),
                        "name": "tool_output",
                        "startTime": timestamp_utc,
                        "endTime": timestamp_utc,
                        "metadata": { "turn_index": turn_index },
                        "output": { "output": tool_output },
                        "level": "DEFAULT"
                    }),
                })
            }
            TraceEvent::FinalResponse { trace_id, turn_index, timestamp_utc, final_response } => {
                // Always create generation for response
                Some(LangfuseEvent {
                    id: envelope_id,
                    timestamp: timestamp.clone(),
                    event_type: "generation-create".to_string(),
                    body: serde_json::json!({
                        "id": format!("{}-turn-{}-response", trace_id, turn_index),
                        "traceId": trace_id.to_string(),
                        "name": "assistant_response",
                        "startTime": timestamp_utc,
                        "endTime": timestamp_utc,
                        "model": "amazon-q",
                        "output": final_response,
                        "metadata": { "turn_index": turn_index }
                    }),
                })
            }
            TraceEvent::UserInterrupt { trace_id, turn_index, timestamp_utc, user_input, .. } => {
                Some(LangfuseEvent {
                    id: envelope_id,
                    timestamp: timestamp.clone(),
                    event_type: "span-create".to_string(),
                    body: serde_json::json!({
                        "id": format!("{}-turn-{}-interrupt", trace_id, turn_index),
                        "traceId": trace_id.to_string(),
                        "name": "user_interrupt",
                        "startTime": timestamp_utc,
                        "endTime": timestamp_utc,
                        "metadata": { "turn_index": turn_index },
                        "input": { "correction": user_input },
                        "level": "WARNING"
                    }),
                })
            }
        }
    }
}

impl Drop for LangfuseSink {
    fn drop(&mut self) {
        let successes = self.success_count.load(Ordering::Relaxed);
        let errors = self.error_count.load(Ordering::Relaxed);
        
        if successes > 0 || errors > 0 {
            eprintln!("\n📊 Langfuse Summary:");
            eprintln!("   Events sent: {}", successes + errors);
            if errors == 0 {
                eprintln!("   ✅ All events delivered");
            } else {
                eprintln!("   ✅ Successful: {}", successes);
                eprintln!("   ❌ Failed: {}", errors);
            }
        }
    }
}
