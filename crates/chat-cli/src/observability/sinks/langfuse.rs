use crate::observability::events::TraceEvent;
use serde::Serialize;
use std::time::Duration;
use tokio::sync::mpsc;

const LANGFUSE_API_URL: &str = "https://cloud.langfuse.com";
const BATCH_SIZE: usize = 1;  // Flush immediately for testing
const FLUSH_INTERVAL_SECS: u64 = 5;
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
}

impl LangfuseSink {
    pub fn new(api_key: String, public_key: String, api_url: Option<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        let (tx, rx) = mpsc::unbounded_channel();
        
        let api_url_final = api_url.unwrap_or_else(|| LANGFUSE_API_URL.to_string());
        
        let sink = Self {
            api_key: api_key.clone(),
            public_key: public_key.clone(),
            _api_url: api_url_final.clone(),
            _client: client.clone(),
            tx,
        };

        tokio::spawn(Self::batch_worker(
            rx,
            client,
            sink.api_key.clone(),
            sink.public_key.clone(),
            api_url_final,
        ));

        Ok(sink)
    }

    pub async fn emit(&self, event: TraceEvent) -> Result<(), Box<dyn std::error::Error>> {
        self.tx.send(event)?;
        Ok(())
    }

    async fn batch_worker(
        mut rx: mpsc::UnboundedReceiver<TraceEvent>,
        client: reqwest::Client,
        api_key: String,
        public_key: String,
        api_url: String,
    ) {
        let mut batch = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_secs(FLUSH_INTERVAL_SECS));

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    if let Some(lf_event) = Self::map_event(event) {
                        batch.push(lf_event);
                        if batch.len() >= BATCH_SIZE {
                            Self::flush_batch(&client, &api_key, &public_key, &api_url, &mut batch).await;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        Self::flush_batch(&client, &api_key, &public_key, &api_url, &mut batch).await;
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
    ) {
        if batch.is_empty() {
            return;
        }

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
                    tracing::debug!("Flushed {} events to Langfuse", payload.batch.len());
                    return;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    tracing::warn!("Langfuse API error (attempt {}): {} - {}", attempt, status, body);
                }
                Err(e) => {
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
                        "input": { "text": user_input },
                        "level": "DEFAULT"
                    }),
                })
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
                        "output": { "text": final_response },
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
