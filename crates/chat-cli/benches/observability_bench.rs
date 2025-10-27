use chat_cli::observability::{TraceCollector, ObservabilityConfig, TraceEvent};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

fn bench_event_emission(c: &mut Criterion) {
    let temp_dir = TempDir::new().unwrap();
    let config = ObservabilityConfig {
        enabled: true,
        output_dir: temp_dir.path().to_path_buf(),
        langfuse_enabled: false,
        langfuse_api_key: None,
        langfuse_api_url: None,
    };

    let collector = TraceCollector::new(config);
    let trace_id = collector.trace_id();

    c.bench_function("emit_user_prompt", |b| {
        b.iter(|| {
            collector.emit(black_box(TraceEvent::UserPrompt {
                trace_id,
                turn_index: 0,
                timestamp_utc: chrono::Utc::now().to_rfc3339(),
                user_input: "Test prompt".to_string(),
            }));
        });
    });

    c.bench_function("emit_final_response", |b| {
        b.iter(|| {
            collector.emit(black_box(TraceEvent::FinalResponse {
                trace_id,
                turn_index: 0,
                timestamp_utc: chrono::Utc::now().to_rfc3339(),
                final_response: "Test response".to_string(),
            }));
        });
    });
}

fn bench_disabled_collector(c: &mut Criterion) {
    let config = ObservabilityConfig {
        enabled: false,
        output_dir: std::path::PathBuf::from("/tmp"),
        langfuse_enabled: false,
        langfuse_api_key: None,
        langfuse_api_url: None,
    };

    let collector = TraceCollector::new(config);
    let trace_id = collector.trace_id();

    c.bench_function("emit_disabled", |b| {
        b.iter(|| {
            collector.emit(black_box(TraceEvent::UserPrompt {
                trace_id,
                turn_index: 0,
                timestamp_utc: chrono::Utc::now().to_rfc3339(),
                user_input: "Test prompt".to_string(),
            }));
        });
    });
}

criterion_group!(benches, bench_event_emission, bench_disabled_collector);
criterion_main!(benches);
