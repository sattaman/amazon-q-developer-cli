use chat_cli::observability::{TraceCollector, ObservabilityConfig, TraceEvent};
use std::path::PathBuf;
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test]
async fn test_full_conversation_flow() {
    let temp_dir = TempDir::new().unwrap();
    let trace_dir = temp_dir.path().to_path_buf();

    let config = ObservabilityConfig {
        enabled: true,
        output_dir: trace_dir.clone(),
        langfuse_enabled: false,
        langfuse_api_key: None,
        langfuse_api_url: None,
    };

    let collector = TraceCollector::new(config);
    let trace_id = collector.trace_id();

    // Simulate conversation flow
    collector.emit(TraceEvent::UserPrompt {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        user_input: "What is the capital of France?".to_string(),
    });

    collector.emit(TraceEvent::AgentThought {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        agent_thought_trace: "I need to provide the capital of France".to_string(),
    });

    collector.emit(TraceEvent::FinalResponse {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        final_response: "The capital of France is Paris.".to_string(),
    });

    // Wait for async writes
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify trace file exists
    let trace_file = trace_dir.join(format!("{}.jsonl", trace_id));
    assert!(trace_file.exists(), "Trace file should exist");

    // Read and verify events
    let content = std::fs::read_to_string(&trace_file).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3, "Should have 3 events");

    // Verify each event is valid JSON
    for line in lines {
        let _: serde_json::Value = serde_json::from_str(line).unwrap();
    }
}

#[tokio::test]
async fn test_tool_execution_flow() {
    let temp_dir = TempDir::new().unwrap();
    let trace_dir = temp_dir.path().to_path_buf();

    let config = ObservabilityConfig {
        enabled: true,
        output_dir: trace_dir.clone(),
        langfuse_enabled: false,
        langfuse_api_key: None,
        langfuse_api_url: None,
    };

    let collector = TraceCollector::new(config);
    let trace_id = collector.trace_id();

    collector.emit(TraceEvent::UserPrompt {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        user_input: "Read the file test.txt".to_string(),
    });

    collector.emit(TraceEvent::ToolExecute {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        tool_calls_executed: vec![chat_cli::observability::ToolCall {
            name: "fs_read".to_string(),
            tool_use_id: "tool-123".to_string(),
            params: serde_json::json!({"path": "test.txt"}),
        }],
    });

    collector.emit(TraceEvent::ToolOutput {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        tool_use_id: "tool-123".to_string(),
        tool_output: "File contents here".to_string(),
    });

    collector.emit(TraceEvent::FinalResponse {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        final_response: "The file contains: File contents here".to_string(),
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let trace_file = trace_dir.join(format!("{}.jsonl", trace_id));
    assert!(trace_file.exists());

    let content = std::fs::read_to_string(&trace_file).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 4, "Should have 4 events");
}

#[tokio::test]
async fn test_multi_turn_conversation() {
    let temp_dir = TempDir::new().unwrap();
    let trace_dir = temp_dir.path().to_path_buf();

    let config = ObservabilityConfig {
        enabled: true,
        output_dir: trace_dir.clone(),
        langfuse_enabled: false,
        langfuse_api_key: None,
        langfuse_api_url: None,
    };

    let collector = TraceCollector::new(config);
    let trace_id = collector.trace_id();

    // Turn 0
    collector.emit(TraceEvent::UserPrompt {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        user_input: "Hello".to_string(),
    });

    collector.emit(TraceEvent::FinalResponse {
        trace_id,
        turn_index: 0,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        final_response: "Hi there!".to_string(),
    });

    // Turn 1
    collector.emit(TraceEvent::UserPrompt {
        trace_id,
        turn_index: 1,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        user_input: "How are you?".to_string(),
    });

    collector.emit(TraceEvent::FinalResponse {
        trace_id,
        turn_index: 1,
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        final_response: "I'm doing well, thanks!".to_string(),
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let trace_file = trace_dir.join(format!("{}.jsonl", trace_id));
    let content = std::fs::read_to_string(&trace_file).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 4, "Should have 4 events across 2 turns");

    // Verify turn indices
    let events: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    assert_eq!(events[0]["turn_index"], 0);
    assert_eq!(events[1]["turn_index"], 0);
    assert_eq!(events[2]["turn_index"], 1);
    assert_eq!(events[3]["turn_index"], 1);
}
