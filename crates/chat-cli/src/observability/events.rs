use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppContext {
    pub app_name: String,
    pub app_version: String,
    pub build_type: String,
    pub source_path: Option<String>,
}

impl Default for AppContext {
    fn default() -> Self {
        Self {
            app_name: std::env::var("Q_APP_NAME").unwrap_or_else(|_| "amazon-q-cli".to_string()),
            app_version: std::env::var("Q_APP_VERSION").unwrap_or_else(|_| "unknown".to_string()),
            build_type: std::env::var("Q_BUILD_TYPE").unwrap_or_else(|_| "release".to_string()),
            source_path: std::env::var("Q_SOURCE_PATH").ok(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum TraceEvent {
    UserPrompt {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        user_input: String,
        app_context: AppContext,
    },
    AgentThought {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        agent_thought_trace: String,
        app_context: AppContext,
    },
    ToolExecute {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        tool_calls_executed: Vec<ToolCall>,
        app_context: AppContext,
    },
    ToolOutput {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        tool_use_id: String,
        tool_output: String,
        app_context: AppContext,
    },
    UserInterrupt {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        interrupt_flag: bool,
        user_input: String,
        app_context: AppContext,
    },
    FinalResponse {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: String,
        final_response: String,
        app_context: AppContext,
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
            app_context: AppContext::default(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("user_prompt"));
        assert!(json.contains("test prompt"));
        assert!(json.contains("app_context"));

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
            app_context: AppContext::default(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("tool_execute"));
        assert!(json.contains("fs_read"));
        assert!(json.contains("/test.txt"));
        assert!(json.contains("app_context"));
    }

    #[test]
    fn test_all_required_fields() {
        let events = vec![
            TraceEvent::UserPrompt {
                trace_id: Uuid::new_v4(),
                turn_index: 0,
                timestamp_utc: "2025-10-27T12:00:00Z".to_string(),
                user_input: "test".to_string(),
                app_context: AppContext::default(),
            },
            TraceEvent::FinalResponse {
                trace_id: Uuid::new_v4(),
                turn_index: 0,
                timestamp_utc: "2025-10-27T12:00:00Z".to_string(),
                final_response: "response".to_string(),
                app_context: AppContext::default(),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            
            assert!(value.get("trace_id").is_some());
            assert!(value.get("turn_index").is_some());
            assert!(value.get("timestamp_utc").is_some());
            assert!(value.get("event_type").is_some());
            assert!(value.get("app_context").is_some());
        }
    }

    #[test]
    fn test_app_context_from_env() {
        std::env::set_var("Q_APP_NAME", "test-app");
        std::env::set_var("Q_APP_VERSION", "1.0.0");
        std::env::set_var("Q_BUILD_TYPE", "test");
        std::env::set_var("Q_SOURCE_PATH", "/test/path");

        let context = AppContext::default();
        assert_eq!(context.app_name, "test-app");
        assert_eq!(context.app_version, "1.0.0");
        assert_eq!(context.build_type, "test");
        assert_eq!(context.source_path, Some("/test/path".to_string()));

        // Clean up
        std::env::remove_var("Q_APP_NAME");
        std::env::remove_var("Q_APP_VERSION");
        std::env::remove_var("Q_BUILD_TYPE");
        std::env::remove_var("Q_SOURCE_PATH");
    }
}
