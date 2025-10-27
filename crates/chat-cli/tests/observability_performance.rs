use chat_cli::observability::{TraceCollector, ObservabilityConfig, TraceEvent};
use std::time::Instant;
use tempfile::TempDir;

#[tokio::test]
async fn test_emission_overhead() {
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

    let iterations = 1000;
    let start = Instant::now();

    for i in 0..iterations {
        collector.emit(TraceEvent::UserPrompt {
            trace_id,
            turn_index: i,
            timestamp_utc: chrono::Utc::now().to_rfc3339(),
            user_input: "Test prompt".to_string(),
        });
    }

    let duration = start.elapsed();
    let avg_micros = duration.as_micros() / iterations as u128;

    println!("Average emission time: {} microseconds", avg_micros);
    println!("Total time for {} events: {:?}", iterations, duration);

    // Verify overhead is minimal (< 100 microseconds per event)
    assert!(avg_micros < 100, "Emission overhead too high: {} μs", avg_micros);

    // Wait for async writes
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
}

#[tokio::test]
async fn test_disabled_collector_overhead() {
    let config = ObservabilityConfig {
        enabled: false,
        output_dir: std::path::PathBuf::from("/tmp"),
        langfuse_enabled: false,
        langfuse_api_key: None,
        langfuse_api_url: None,
    };

    let collector = TraceCollector::new(config);
    let trace_id = collector.trace_id();

    let iterations = 10000;
    let start = Instant::now();

    for i in 0..iterations {
        collector.emit(TraceEvent::UserPrompt {
            trace_id,
            turn_index: i,
            timestamp_utc: chrono::Utc::now().to_rfc3339(),
            user_input: "Test prompt".to_string(),
        });
    }

    let duration = start.elapsed();
    let avg_nanos = duration.as_nanos() / iterations as u128;

    println!("Average disabled emission time: {} nanoseconds", avg_nanos);

    // Disabled collector should have near-zero overhead (< 1 microsecond)
    assert!(avg_nanos < 1000, "Disabled collector overhead too high: {} ns", avg_nanos);
}
