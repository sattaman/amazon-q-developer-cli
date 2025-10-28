use chat_cli::observability::events::{TraceEvent, ToolCall};
use serde_json::Value;
use uuid::Uuid;

// Test the CoT hierarchy mapping logic
fn map_agent_coordinator_span(trace_id: Uuid, turn_index: u32, timestamp: &str) -> Value {
    serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "type": "span-create",
        "body": {
            "id": format!("{}-turn-{}-agent", trace_id, turn_index),
            "traceId": trace_id.to_string(),
            "name": "reasoning_coordinator",
            "as_type": "agent",
            "startTime": timestamp,
            "endTime": timestamp,
            "metadata": { 
                "turn_index": turn_index,
                "role": "coordinator",
                "reasoning_strategy": "chain_of_thought"
            },
            "level": "INFO"
        }
    })
}

fn map_chain_span(trace_id: Uuid, turn_index: u32, timestamp: &str, thought: &str) -> Value {
    let agent_span_id = format!("{}-turn-{}-agent", trace_id, turn_index);
    let chain_span_id = format!("{}-turn-{}-chain", trace_id, turn_index);
    
    serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "type": "span-create",
        "body": {
            "id": chain_span_id,
            "traceId": trace_id.to_string(),
            "parentObservationId": agent_span_id,
            "name": "reasoning_step",
            "as_type": "chain",
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
    })
}

fn map_tool_span(trace_id: Uuid, turn_index: u32, timestamp: &str, tool_calls: &[ToolCall]) -> Value {
    let tool_name = &tool_calls[0].name;
    let agent_span_id = format!("{}-turn-{}-agent", trace_id, turn_index);
    
    serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "type": "span-create",
        "body": {
            "id": format!("{}-turn-{}-tool-{}", trace_id, turn_index, tool_name),
            "traceId": trace_id.to_string(),
            "parentObservationId": agent_span_id,
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
    })
}

#[test]
fn test_cot_hierarchy_structure() {
    let trace_id = Uuid::new_v4();
    let timestamp = chrono::Utc::now().to_rfc3339();
    
    // Create CoT hierarchy
    let agent_span = map_agent_coordinator_span(trace_id, 0, &timestamp);
    let chain_span = map_chain_span(trace_id, 0, &timestamp, "I need to analyze this request");
    let tool_span = map_tool_span(trace_id, 0, &timestamp, &[ToolCall {
        name: "fs_read".to_string(),
        tool_use_id: "tool-123".to_string(),
        params: serde_json::json!({"path": "test.txt"}),
    }]);
    
    // Verify agent coordinator
    assert_eq!(agent_span["body"]["as_type"], "agent");
    assert_eq!(agent_span["body"]["name"], "reasoning_coordinator");
    assert_eq!(agent_span["body"]["metadata"]["role"], "coordinator");
    
    // Verify chain span has correct parent
    assert_eq!(chain_span["body"]["as_type"], "chain");
    assert_eq!(chain_span["body"]["name"], "reasoning_step");
    assert_eq!(chain_span["body"]["parentObservationId"], 
               format!("{}-turn-0-agent", trace_id));
    
    // Verify tool span has correct parent
    assert_eq!(tool_span["body"]["as_type"], "tool");
    assert_eq!(tool_span["body"]["name"], "tool_fs_read");
    assert_eq!(tool_span["body"]["parentObservationId"], 
               format!("{}-turn-0-agent", trace_id));
}

#[test]
fn test_parent_child_relationships() {
    let trace_id = Uuid::new_v4();
    let timestamp = chrono::Utc::now().to_rfc3339();
    
    let agent_span = map_agent_coordinator_span(trace_id, 0, &timestamp);
    let chain_span = map_chain_span(trace_id, 0, &timestamp, "Step 1");
    let tool_span = map_tool_span(trace_id, 0, &timestamp, &[ToolCall {
        name: "test_tool".to_string(),
        tool_use_id: "tool-456".to_string(),
        params: serde_json::json!({}),
    }]);
    
    let agent_id = agent_span["body"]["id"].as_str().unwrap();
    let chain_parent = chain_span["body"]["parentObservationId"].as_str().unwrap();
    let tool_parent = tool_span["body"]["parentObservationId"].as_str().unwrap();
    
    // Both chain and tool should have agent as parent
    assert_eq!(chain_parent, agent_id);
    assert_eq!(tool_parent, agent_id);
    
    // Verify trace ID consistency
    assert_eq!(agent_span["body"]["traceId"], trace_id.to_string());
    assert_eq!(chain_span["body"]["traceId"], trace_id.to_string());
    assert_eq!(tool_span["body"]["traceId"], trace_id.to_string());
}

#[test]
fn test_multi_turn_cot_isolation() {
    let trace_id = Uuid::new_v4();
    let timestamp = chrono::Utc::now().to_rfc3339();
    
    // Turn 0 spans
    let agent_span_0 = map_agent_coordinator_span(trace_id, 0, &timestamp);
    let chain_span_0 = map_chain_span(trace_id, 0, &timestamp, "Turn 0 thought");
    
    // Turn 1 spans
    let agent_span_1 = map_agent_coordinator_span(trace_id, 1, &timestamp);
    let chain_span_1 = map_chain_span(trace_id, 1, &timestamp, "Turn 1 thought");
    
    // Verify different agent coordinators for different turns
    assert_ne!(agent_span_0["body"]["id"], agent_span_1["body"]["id"]);
    
    // Verify chain spans reference correct parent agents
    assert_eq!(chain_span_0["body"]["parentObservationId"], 
               format!("{}-turn-0-agent", trace_id));
    assert_eq!(chain_span_1["body"]["parentObservationId"], 
               format!("{}-turn-1-agent", trace_id));
    
    // Verify turn index metadata
    assert_eq!(agent_span_0["body"]["metadata"]["turn_index"], 0);
    assert_eq!(agent_span_1["body"]["metadata"]["turn_index"], 1);
}

#[test]
fn test_cot_semantic_types() {
    let trace_id = Uuid::new_v4();
    let timestamp = chrono::Utc::now().to_rfc3339();
    
    let agent_span = map_agent_coordinator_span(trace_id, 0, &timestamp);
    let chain_span = map_chain_span(trace_id, 0, &timestamp, "Analysis step");
    let tool_span = map_tool_span(trace_id, 0, &timestamp, &[ToolCall {
        name: "fs_write".to_string(),
        tool_use_id: "tool-789".to_string(),
        params: serde_json::json!({"path": "output.txt", "content": "test"}),
    }]);
    
    // Verify all semantic types are correct for agent graph generation
    assert_eq!(agent_span["body"]["as_type"], "agent");
    assert_eq!(chain_span["body"]["as_type"], "chain");
    assert_eq!(tool_span["body"]["as_type"], "tool");
    
    // Verify names follow convention
    assert_eq!(agent_span["body"]["name"], "reasoning_coordinator");
    assert_eq!(chain_span["body"]["name"], "reasoning_step");
    assert_eq!(tool_span["body"]["name"], "tool_fs_write");
}

#[test]
fn test_cot_metadata_enrichment() {
    let trace_id = Uuid::new_v4();
    let timestamp = chrono::Utc::now().to_rfc3339();
    
    let agent_span = map_agent_coordinator_span(trace_id, 2, &timestamp);
    let chain_span = map_chain_span(trace_id, 2, &timestamp, "Complex reasoning");
    
    // Verify agent metadata
    let agent_meta = &agent_span["body"]["metadata"];
    assert_eq!(agent_meta["turn_index"], 2);
    assert_eq!(agent_meta["role"], "coordinator");
    assert_eq!(agent_meta["reasoning_strategy"], "chain_of_thought");
    
    // Verify chain metadata
    let chain_meta = &chain_span["body"]["metadata"];
    assert_eq!(chain_meta["turn_index"], 2);
    assert_eq!(chain_meta["reasoning_strategy"], "chain_of_thought");
    assert_eq!(chain_meta["step_type"], "analysis");
}
