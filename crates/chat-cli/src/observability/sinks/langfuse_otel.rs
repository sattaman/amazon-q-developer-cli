use crate::observability::events::TraceEvent;
use opentelemetry::{global, trace::{Span, TraceContextExt, Tracer, TracerProvider as _}, Context, KeyValue};
use opentelemetry_langfuse::ExporterBuilder;
use opentelemetry_sdk::{runtime::Tokio, trace::{SdkTracerProvider, span_processor_with_async_runtime::BatchSpanProcessor}, Resource};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct LangfuseOtelSink {
    provider: Arc<SdkTracerProvider>,
    tracer: opentelemetry_sdk::trace::Tracer,
    active_contexts: Arc<Mutex<HashMap<String, Context>>>,
}

impl LangfuseOtelSink {
    pub fn new(api_key: String, public_key: String, api_url: Option<String>) -> Result<Self, Box<dyn std::error::Error>> {
        // Set environment variables for ExporterBuilder
        // SAFETY: set_var is unsafe because it can cause data races if other threads
        // are reading environment variables. We call this during initialization before
        // spawning any worker threads, making it safe in this context.
        unsafe {
            std::env::set_var("LANGFUSE_SECRET_KEY", &api_key);
            std::env::set_var("LANGFUSE_PUBLIC_KEY", &public_key);
            if let Some(url) = &api_url {
                std::env::set_var("LANGFUSE_HOST", url);
            }
        }

        // Create exporter
        let exporter = ExporterBuilder::from_env()?.build()?;

        // Build tracer provider with BatchSpanProcessor
        let provider = SdkTracerProvider::builder()
            .with_resource(
                Resource::builder()
                    .with_attributes(vec![KeyValue::new("service.name", "amazon-q-cli")])
                    .build()
            )
            .with_span_processor(BatchSpanProcessor::builder(exporter, Tokio).build())
            .build();

        let tracer = provider.tracer("amazon-q-cli");

        // Set as global provider
        global::set_tracer_provider(provider.clone());

        eprintln!("✅ Langfuse OpenTelemetry sink initialized");

        Ok(Self {
            provider: Arc::new(provider),
            tracer,
            active_contexts: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn emit(&self, event: TraceEvent) -> Result<(), Box<dyn std::error::Error>> {
        match event {
            TraceEvent::UserPrompt { trace_id, user_input, .. } => {
                // Create root trace span - this will be the parent for all other spans
                let span = self.tracer.span_builder("q-chat-session")
                    .with_attributes(vec![
                        KeyValue::new("langfuse.trace.name", "q-chat-session"),
                        KeyValue::new("langfuse.trace.input", user_input),
                    ])
                    .start(&self.tracer);
                let ctx = Context::current().with_span(span);
                self.active_contexts.lock().unwrap().insert(format!("trace-{}", trace_id), ctx);
            }
            TraceEvent::AgentThought { trace_id, turn_index, agent_thought_trace, .. } => {
                // Get parent trace context - CRITICAL: use the same trace context
                let parent_ctx = self.active_contexts.lock().unwrap()
                    .get(&format!("trace-{}", trace_id))
                    .cloned()
                    .unwrap_or_else(Context::current);
                
                // Create child span within the same trace - USE SEMANTIC TYPE
                let mut span = self.tracer.span_builder(format!("reasoning-{}-{}", trace_id, turn_index))
                    .with_attributes(vec![
                        KeyValue::new("langfuse.observation.type", "agent"),  // Semantic type for agent graphs
                        KeyValue::new("langfuse.observation.input", "Chain of thought reasoning"),
                        KeyValue::new("langfuse.observation.output", agent_thought_trace),
                    ])
                    .start_with_context(&self.tracer, &parent_ctx);
                span.end();
            }
            TraceEvent::FinalResponse { trace_id, turn_index, final_response, .. } => {
                // Get parent trace context - CRITICAL: use the same trace context
                let parent_ctx = self.active_contexts.lock().unwrap()
                    .get(&format!("trace-{}", trace_id))
                    .cloned()
                    .unwrap_or_else(Context::current);
                
                // Create child span within the same trace
                let mut span = self.tracer.span_builder(format!("generation-{}-{}", trace_id, turn_index))
                    .with_attributes(vec![
                        KeyValue::new("langfuse.observation.type", "generation"),
                        KeyValue::new("langfuse.observation.output", final_response.clone()),
                        KeyValue::new("gen_ai.response.model", "amazon-q"),
                    ])
                    .start_with_context(&self.tracer, &parent_ctx);
                span.end();
                
                // Update trace-level output
                if let Some(ctx) = self.active_contexts.lock().unwrap().get(&format!("trace-{}", trace_id)) {
                    let trace_span = ctx.span();
                    trace_span.set_attribute(KeyValue::new("langfuse.trace.output", final_response));
                }
            }
            TraceEvent::ToolExecute { trace_id, turn_index, tool_calls_executed, .. } => {
                if let Some(tool) = tool_calls_executed.first() {
                    // Get parent trace context - CRITICAL: use the same trace context
                    let parent_ctx = self.active_contexts.lock().unwrap()
                        .get(&format!("trace-{}", trace_id))
                        .cloned()
                        .unwrap_or_else(Context::current);
                    
                    // Create child span within the same trace - USE SEMANTIC TYPE
                    let span = self.tracer.span_builder(format!("tool-{}", tool.name))
                        .with_attributes(vec![
                            KeyValue::new("langfuse.observation.type", "tool"),  // Semantic type for tools
                            KeyValue::new("langfuse.observation.name", tool.name.clone()),
                            KeyValue::new("langfuse.observation.input", serde_json::to_string(&tool_calls_executed)?),
                        ])
                        .start_with_context(&self.tracer, &parent_ctx);
                    
                    // CRITICAL FIX: Store the span within the parent context, not a new context
                    let tool_ctx = parent_ctx.with_span(span);
                    self.active_contexts.lock().unwrap().insert(format!("tool-{}-{}", trace_id, tool.tool_use_id), tool_ctx);
                }
            }
            TraceEvent::ToolOutput { trace_id, tool_use_id, tool_output, .. } => {
                if let Some(ctx) = self.active_contexts.lock().unwrap().remove(&format!("tool-{}-{}", trace_id, tool_use_id)) {
                    let span = ctx.span();
                    span.set_attribute(KeyValue::new("langfuse.observation.output", tool_output));
                    span.end();
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn flush(&self) {
        eprintln!("🔄 Flushing Langfuse spans...");
        let mut contexts = self.active_contexts.lock().unwrap();
        let count = contexts.len();
        for (_, ctx) in contexts.drain() {
            ctx.span().end();
        }
        drop(contexts);
        eprintln!("   Ended {} active spans", count);
        
        if let Err(e) = self.provider.force_flush() {
            eprintln!("❌ Langfuse flush error: {}", e);
        } else {
            eprintln!("   force_flush() completed");
        }
        
        // Wait for async export to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        eprintln!("✅ Langfuse flush complete");
    }
}

impl Drop for LangfuseOtelSink {
    fn drop(&mut self) {
        eprintln!("📊 Langfuse OpenTelemetry sink shutting down");
        let _ = self.provider.shutdown();
    }
}
