use super::{config::ObservabilityConfig, events::TraceEvent, sinks::jsonl::JsonlSink, sinks::langfuse_otel::LangfuseOtelSink};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct TraceCollector {
    trace_id: Uuid,
    turn_index: Arc<std::sync::atomic::AtomicU32>,
    tx: mpsc::UnboundedSender<TraceEvent>,
    langfuse_sink: Option<Arc<LangfuseOtelSink>>,
}

impl TraceCollector {
    pub fn new(config: ObservabilityConfig) -> Self {
        let trace_id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::unbounded_channel();

        if config.enabled {
            let sink = JsonlSink::new(config.output_dir.clone(), trace_id);
            let langfuse_sink = if let (Some(secret_key), Some(public_key)) = 
                (&config.langfuse_api_key, &config.langfuse_public_key) {
                eprintln!("🔗 Initializing Langfuse OpenTelemetry sink...");
                match LangfuseOtelSink::new(
                    secret_key.clone(), 
                    public_key.clone(), 
                    config.langfuse_api_url.clone()
                ) {
                    Ok(sink) => Some(Arc::new(sink)),
                    Err(e) => {
                        eprintln!("⚠️  Failed to initialize Langfuse: {}", e);
                        tracing::error!("Failed to create Langfuse sink: {}", e);
                        None
                    }
                }
            } else {
                eprintln!("❌ Langfuse not configured (missing API keys)");
                None
            };

            let langfuse_clone = langfuse_sink.clone();
            
            tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    // Write to JSONL
                    if let Err(e) = sink.write(&event).await {
                        tracing::error!("Failed to write trace event: {}", e);
                    }
                    
                    // Send to Langfuse
                    if let Some(ref lf) = langfuse_clone {
                        if let Err(e) = lf.emit(event.clone()).await {
                            tracing::error!("Failed to emit to Langfuse: {}", e);
                        }
                    }
                }
            });
            
            Self {
                trace_id,
                turn_index: Arc::new(std::sync::atomic::AtomicU32::new(0)),
                tx,
                langfuse_sink,
            }
        } else {
            Self {
                trace_id,
                turn_index: Arc::new(std::sync::atomic::AtomicU32::new(0)),
                tx,
                langfuse_sink: None,
            }
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
    
    pub async fn flush(&self) {
        eprintln!("🔄 Collector flush called");
        if let Some(ref lf) = self.langfuse_sink {
            eprintln!("   Calling Langfuse sink flush...");
            lf.flush().await;
        } else {
            eprintln!("   No Langfuse sink to flush");
        }
        eprintln!("✅ Collector flush complete");
    }
}
