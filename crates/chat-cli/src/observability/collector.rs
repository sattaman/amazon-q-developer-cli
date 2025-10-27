use super::{config::ObservabilityConfig, events::TraceEvent, sinks::jsonl::JsonlSink, sinks::langfuse::LangfuseSink};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct TraceCollector {
    trace_id: Uuid,
    turn_index: Arc<std::sync::atomic::AtomicU32>,
    tx: mpsc::UnboundedSender<TraceEvent>,
}

impl TraceCollector {
    pub fn new(config: ObservabilityConfig) -> Self {
        let trace_id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::unbounded_channel();

        if config.enabled {
            let sink = JsonlSink::new(config.output_dir.clone(), trace_id);
            let langfuse_sink = if let (Some(secret_key), Some(public_key)) = 
                (&config.langfuse_api_key, &config.langfuse_public_key) {
                match LangfuseSink::new(
                    secret_key.clone(), 
                    public_key.clone(), 
                    config.langfuse_api_url.clone()
                ) {
                    Ok(sink) => Some(sink),
                    Err(e) => {
                        tracing::error!("Failed to create Langfuse sink: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    if let Err(e) = sink.write(&event).await {
                        tracing::error!("Failed to write trace event: {}", e);
                    }
                    if let Some(ref lf) = langfuse_sink {
                        if let Err(e) = lf.emit(event.clone()).await {
                            tracing::error!("Failed to emit to Langfuse: {}", e);
                        }
                    }
                }
            });
        }

        Self {
            trace_id,
            turn_index: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            tx,
        }
    }

    pub fn trace_id(&self) -> Uuid {
        self.trace_id
    }

    pub fn current_turn(&self) -> u32 {
        self.turn_index
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn increment_turn(&self) {
        self.turn_index
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn emit(&self, event: TraceEvent) {
        let _ = self.tx.send(event);
    }
}
