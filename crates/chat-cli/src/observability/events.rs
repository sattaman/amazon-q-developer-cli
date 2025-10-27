use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum TraceEvent {
    UserPrompt {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        user_input: String,
    },
    AgentThought {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        agent_thought_trace: String,
    },
    ToolExecute {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        tool_calls_executed: Vec<ToolCall>,
    },
    ToolOutput {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        tool_use_id: String,
        tool_output: String,
    },
    UserInterrupt {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        interrupt_flag: bool,
        user_input: String,
    },
    FinalResponse {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        final_response: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub tool_use_id: String,
    pub params: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_prompt_serialization() {
        let event = TraceEvent::UserPrompt {
            trace_id: Uuid::new_v4(),
            turn_index: 0,
            timestamp_utc: "2025-10-27T12:00:00Z".to_string(),
            user_input: "test prompt".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("user_prompt"));
        assert!(json.contains("test prompt"));

        let parsed: TraceEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            TraceEvent::UserPrompt { user_input, .. } => assert_eq!(user_input, "test prompt"),
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_tool_execute_serialization() {
        let tool_call = ToolCall {
            name: "fs_read".to_string(),
            tool_use_id: "test_id".to_string(),
            params: serde_json::json!({"path": "/test.txt"}),
        };
        
        let event = TraceEvent::ToolExecute {
            trace_id: Uuid::new_v4(),
            turn_index: 0,
            timestamp_utc: "2025-10-27T12:00:00Z".to_string(),
            tool_calls_executed: vec![tool_call],
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("tool_execute"));
        assert!(json.contains("fs_read"));
        assert!(json.contains("/test.txt"));
    }

    #[test]
    fn test_all_required_fields() {
        let events = vec![
            TraceEvent::UserPrompt {
                trace_id: Uuid::new_v4(),
                turn_index: 0,
                timestamp_utc: "2025-10-27T12:00:00Z".to_string(),
                user_input: "test".to_string(),
            },
            TraceEvent::FinalResponse {
                trace_id: Uuid::new_v4(),
                turn_index: 0,
                timestamp_utc: "2025-10-27T12:00:00Z".to_string(),
                final_response: "response".to_string(),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            
            assert!(value.get("trace_id").is_some());
            assert!(value.get("turn_index").is_some());
            assert!(value.get("timestamp_utc").is_some());
            assert!(value.get("event_type").is_some());
        }
    }
}
