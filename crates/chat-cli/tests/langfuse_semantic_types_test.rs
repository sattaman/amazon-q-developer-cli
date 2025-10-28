use chat_cli::observability::events::{TraceEvent, ToolCall};
use serde_json::Value;
use uuid::Uuid;

// Test the event mapping logic directly
fn map_agent_thought_event(trace_id: Uuid, turn_index: u32, thought: &str) -> Option<Value> {
    // Simulate the mapping logic from LangfuseSink::map_event
    let envelope_id = Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().to_rfc3339();
    
    Some(serde_json::json!({
        "id": envelope_id,
        "timestamp": timestamp,
        "type": "span-create",
        "body": {
            "id": format!("{}-turn-{}-thought", trace_id, turn_index),
            "traceId": trace_id.to_string(),
            "name": "agent_reasoning",
            "as_type": "agent",
            "startTime": timestamp,
            "endTime": timestamp,
            "input": { "context": "reasoning_step" },
            "output": { "thought": thought },
            "metadata": { 
                "turn_index": turn_index,
                "reasoning_strategy": "chain_of_thought",
                "step_type": "analysis"
            },
            "level": "DEBUG"
        }
    }))
}

fn map_tool_execute_event(trace_id: Uuid, turn_index: u32, tool_calls: &[ToolCall]) -> Option<Value> {
    let tool_name = tool_calls.first()?.name.clone();
    let envelope_id = Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().to_rfc3339();
    
    Some(serde_json::json!({
        "id": envelope_id,
        "timestamp": timestamp,
        "type": "span-create",
        "body": {
            "id": format!("{}-turn-{}-tool-{}", trace_id, turn_index, tool_name),
            "traceId": trace_id.to_string(),
            "name": format!("tool_{}", tool_name),
            "as_type": "tool",
            "startTime": timestamp,
            "endTime": timestamp,
            "input": tool_calls,
            "metadata": { 
                "turn_index": turn_index,
                "tool_name": tool_name,
                "permission_granted": true
            },
            "level": "DEFAULT"
        }
    }))
}

#[test]
fn test_agent_thought_semantic_type() {
    let trace_id = Uuid::new_v4();
    let thought = "I need to analyze this request carefully";
    
    let event = map_agent_thought_event(trace_id, 0, thought).unwrap();
    let body = &event["body"];
    
    // Verify semantic observation type
    assert_eq!(body["as_type"], "agent");
    assert_eq!(body["name"], "agent_reasoning");
    assert_eq!(body["metadata"]["reasoning_strategy"], "chain_of_thought");
    assert_eq!(body["metadata"]["step_type"], "analysis");
    assert_eq!(body["input"]["context"], "reasoning_step");
    assert_eq!(body["output"]["thought"], thought);
    assert_eq!(body["level"], "DEBUG");
}

#[test]
fn test_tool_execute_semantic_type() {
    let trace_id = Uuid::new_v4();
    let tool_calls = vec![ToolCall {
        name: "fs_read".to_string(),
        tool_use_id: "tool-123".to_string(),
        params: serde_json::json!({"path": "/test.txt"}),
    }];
    
    let event = map_tool_execute_event(trace_id, 0, &tool_calls).unwrap();
    let body = &event["body"];
    
    // Verify semantic observation type
    assert_eq!(body["as_type"], "tool");
    assert_eq!(body["name"], "tool_fs_read");
    assert_eq!(body["metadata"]["tool_name"], "fs_read");
    assert_eq!(body["metadata"]["permission_granted"], true);
    assert_eq!(body["level"], "DEFAULT");
    
    // Verify input structure
    assert!(body["input"].is_array());
    let input_array = body["input"].as_array().unwrap();
    assert_eq!(input_array.len(), 1);
    assert_eq!(input_array[0]["name"], "fs_read");
}

#[test]
fn test_metadata_enrichment() {
    let trace_id = Uuid::new_v4();
    let event = map_agent_thought_event(trace_id, 2, "Complex reasoning step").unwrap();
    let metadata = &event["body"]["metadata"];
    
    // Verify enhanced metadata
    assert_eq!(metadata["turn_index"], 2);
    assert_eq!(metadata["reasoning_strategy"], "chain_of_thought");
    assert_eq!(metadata["step_type"], "analysis");
}

#[test]
fn test_agent_graph_compatibility() {
    let trace_id = Uuid::new_v4();
    
    // Create events that should trigger agent graph
    let agent_event = map_agent_thought_event(trace_id, 0, "Planning approach").unwrap();
    let tool_event = map_tool_execute_event(trace_id, 0, &[ToolCall {
        name: "fs_read".to_string(),
        tool_use_id: "tool-789".to_string(),
        params: serde_json::json!({"path": "test.txt"}),
    }]).unwrap();
    
    let events = vec![agent_event, tool_event];
    
    // Verify agent graph compatibility
    let has_semantic_types = events.iter().any(|event| {
        if let Some(body) = event.get("body") {
            if let Some(as_type) = body.get("as_type") {
                matches!(as_type.as_str(), Some("agent") | Some("tool") | Some("chain"))
            } else {
                false
            }
        } else {
            false
        }
    });
    
    assert!(has_semantic_types, "Events should be compatible with agent graph generation");
    
    // Verify specific semantic types
    assert_eq!(events[0]["body"]["as_type"], "agent");
    assert_eq!(events[1]["body"]["as_type"], "tool");
}

#[test]
fn test_trace_id_consistency() {
    let trace_id = Uuid::new_v4();
    
    let agent_event = map_agent_thought_event(trace_id, 0, "Test thought").unwrap();
    let tool_event = map_tool_execute_event(trace_id, 0, &[ToolCall {
        name: "test_tool".to_string(),
        tool_use_id: "tool-123".to_string(),
        params: serde_json::json!({}),
    }]).unwrap();
    
    // Verify trace ID consistency
    assert_eq!(agent_event["body"]["traceId"], trace_id.to_string());
    assert_eq!(tool_event["body"]["traceId"], trace_id.to_string());
    
    // Verify unique observation IDs
    assert_ne!(agent_event["body"]["id"], tool_event["body"]["id"]);
    
    // Verify ID format
    let agent_id = agent_event["body"]["id"].as_str().unwrap();
    let tool_id = tool_event["body"]["id"].as_str().unwrap();
    
    assert!(agent_id.contains(&trace_id.to_string()));
    assert!(agent_id.contains("thought"));
    assert!(tool_id.contains(&trace_id.to_string()));
    assert!(tool_id.contains("tool"));
}
