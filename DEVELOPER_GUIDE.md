# Amazon Q CLI Observability - Developer Guide

## Quick Reference

### Critical Discovery: CoT Already Available
**File**: `crates/amzn-qdeveloper-streaming-client/src/types/_reasoning_content_event.rs`

```rust
pub struct ReasoningContentEvent {
    pub text: Option<String>,  // ← Chain of Thought data
}
```

**Status**: ✅ Already in stream, currently discarded in parser  
**Action**: Add match arm in `parser.rs:420` to capture

### 5 Integration Points

| Point | File | Function | Captures |
|-------|------|----------|----------|
| 1. Stream Events | `parser.rs` | `ResponseStreamParser::next()` ~350 | CoT, tool proposals, responses |
| 2. User Input | `mod.rs` | `ChatState::HandleInput` | User prompts, turn init |
| 3. Interrupts | `reply.rs` | `ReplyArgs::execute()` ~70 | `/reply` corrections |
| 4. Tool Execution | `parser.rs` | `parse_tool_use()` ~473 | Tool params & results |
| 5. Session | `mod.rs` | Chat session creation | trace_id, turn_index |

---

## Architecture

### Streaming Protocol
- Event-based protocol (similar to Lex V2's `StartConversationResponseEventStream`)
- API: `codewhisperer:CreateTaskAssistConversation`
- HTTP/2 persistent connection
- Client abstracts intermediate events (CoT, tool proposals)

### Technology Stack
- **Language**: Rust
- **SDK**: AWS SDK for Rust
- **Logging**: `tracing` crate (`RUST_LOG` env var)
- **Serialization**: `serde` + `serde_json`

### Project Structure

```
crates/
├── chat-cli/
│   └── src/
│       ├── cli/chat/
│       │   ├── mod.rs              # Main chat loop (179KB)
│       │   ├── parser.rs           # Stream event parser
│       │   ├── tool_manager.rs     # Tool execution (95KB)
│       │   └── cli/reply.rs        # /reply command
│       └── observability/
│           ├── mod.rs              # TraceCollector
│           ├── events.rs           # TraceEvent enum
│           ├── collector.rs        # Session management
│           └── sinks/
│               ├── jsonl.rs        # Local file sink
│               └── langfuse.rs     # Langfuse API sink
│
└── amzn-qdeveloper-streaming-client/
    └── src/types/
        ├── _chat_response_stream.rs        # Main event enum
        ├── _reasoning_content_event.rs     # CoT events
        ├── _tool_use_event.rs              # Tool proposals
        └── _tool_result_event.rs           # Tool results
```

---

## Event Stream Details

### ChatResponseStream Event Types

```rust
pub enum ChatResponseStream {
    // TEXT
    AssistantResponseEvent(AssistantResponseEvent),
    
    // REASONING (CoT) - THE GOLD
    ReasoningContentEvent(ReasoningContentEvent),
    
    // TOOLS
    ToolUseEvent(ToolUseEvent),
    ToolResultEvent(ToolResultEvent),
    
    // METADATA
    MessageMetadataEvent(MessageMetadataEvent),
    MetadataEvent(MetadataEvent),
    
    // SUPPLEMENTARY
    CitationEvent(CitationEvent),
    CodeEvent(CodeEvent),
    FollowupPromptEvent(FollowupPromptEvent),
    
    // ERROR
    InvalidStateEvent(InvalidStateEvent),
    
    Unknown,
}
```

### Event Handling Status

| Event Type | Purpose | Currently Handled? | ACE Mapping |
|------------|---------|-------------------|-------------|
| `ReasoningContentEvent` | Chain of Thought | ❌ **NO** (discarded) | `agent_thought_trace` |
| `ToolUseEvent` | Tool proposals | ✅ Yes | `tool_calls_executed` |
| `ToolResultEvent` | Tool outputs | ⚠️ Partial | `tool_output` |
| `AssistantResponseEvent` | Text responses | ✅ Yes | `final_response` |
| `MessageMetadataEvent` | Metadata | ⚠️ Partial | `trace_id`, timestamps |

---

## Implementation Details

### 1. Stream Event Capture (HIGHEST PRIORITY)

**File**: `crates/chat-cli/src/cli/chat/parser.rs`  
**Function**: `ResponseStreamParser::next_event()` (line ~400-550)

**Current Code**:
```rust
match output {
    ChatResponseStream::AssistantResponseEvent { content } => {
        self.assistant_text.push_str(&content);
        return Ok(ResponseEvent::AssistantText(content));
    },
    ChatResponseStream::ToolUseEvent { tool_use_id, name, input, stop } => {
        // Tool use parsing logic
        return Ok(ResponseEvent::ToolUseStart { name });
    },
    _ => {},  // ← ReasoningContentEvent falls through here and is DISCARDED
}
```

**Hook Location**: Add match arm before `_ => {}`

**What to Capture**:
```rust
ChatResponseStream::ReasoningContentEvent(event) => {
    if let Some(cot_text) = event.text() {
        trace_collector.log_agent_thought(cot_text);
    }
},
```

### 2. Tool Parsing (Streaming JSON)

**File**: `crates/chat-cli/src/cli/chat/parser.rs:473`

**Challenge**: Tool parameters arrive in chunks:
```rust
ToolUseEvent { input: Some("{\"file"), stop: false }
ToolUseEvent { input: Some("\":\"test."), stop: false }
ToolUseEvent { input: Some("txt\"}"), stop: true }
```

**Current Solution** (already implemented):
```rust
async fn parse_tool_use(&mut self, id: String, name: String) -> Result<AssistantToolUse, RecvError> {
    let mut tool_string = String::new();
    
    // Accumulate streaming JSON
    while let Some(ChatResponseStream::ToolUseEvent { .. }) = self.peek().await? {
        if let Some(ChatResponseStream::ToolUseEvent { input, stop, .. }) = self.next().await? {
            if let Some(i) = input {
                tool_string.push_str(&i);
            }
            if let Some(true) = stop {
                break;
            }
        }
    }
    
    // Deserialize accumulated JSON
    let args = serde_json::from_str(&tool_string)?;
    // ← INJECT HERE: Log complete tool_string
}
```

### 3. Sensitive Data Redaction

**Problem**: Event types have custom Debug implementations that hide data:

```rust
impl Debug for ReasoningContentEvent {
    fn fmt(&self, f: &mut Formatter) -> Result {
        formatter.field("text", &"*** Sensitive Data Redacted ***");
    }
}
```

**Solution**: Use `serde_json::to_string()` BEFORE Debug formatting:

```rust
// ✅ CORRECT
let json = serde_json::to_string(&event)?;
trace!("Event: {}", json);

// ❌ WRONG - will show "*** Sensitive Data Redacted ***"
trace!("Event: {:?}", event);
```

---

## Data Structures

### TraceEvent Enum

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum TraceEvent {
    UserPrompt {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: DateTime<Utc>,
        user_input: String,
    },
    AgentThought {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: DateTime<Utc>,
        agent_thought_trace: String,
    },
    ToolExecute {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: DateTime<Utc>,
        tool_calls_executed: Vec<ToolCall>,
    },
    ToolOutput {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: DateTime<Utc>,
        tool_use_id: String,
        tool_output: String,
    },
    UserInterrupt {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: DateTime<Utc>,
        interrupt_flag: bool,
        user_input: String,
    },
    FinalResponse {
        trace_id: Uuid,
        turn_index: u32,
        timestamp_utc: DateTime<Utc>,
        final_response: String,
    },
}
```

### Langfuse Event Format

Events use envelope format:
```json
{
  "id": "envelope-uuid",
  "timestamp": "2025-10-27T17:00:00.000Z",
  "type": "span-create",
  "body": {
    "id": "span-uuid",
    "traceId": "trace-uuid",
    "name": "event_name",
    "input": "...",
    "output": "..."
  }
}
```

---

## Configuration

### CLI Flags

```rust
#[derive(Debug, Args)]
pub struct ChatArgs {
    /// Enable observability tracing
    #[arg(long, env = "Q_TRACE")]
    pub trace: bool,
    
    /// Custom trace output directory
    #[arg(long, env = "Q_TRACE_DIR")]
    pub trace_dir: Option<PathBuf>,
    
    /// Enable Langfuse integration
    #[arg(long, env = "Q_LANGFUSE")]
    pub langfuse: bool,
}
```

### Environment Variables

```bash
# Enable tracing
export Q_TRACE=true
export Q_TRACE_DIR=~/.q/traces

# Langfuse
export Q_LANGFUSE=true
export LANGFUSE_PUBLIC_KEY=pk-...
export LANGFUSE_SECRET_KEY=sk-...
export LANGFUSE_HOST=https://cloud.langfuse.com
```

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_trace_event_serialization() {
        let event = TraceEvent::UserPrompt {
            trace_id: Uuid::new_v4(),
            turn_index: 0,
            timestamp_utc: Utc::now(),
            user_input: "test".to_string(),
        };
        
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("user_prompt"));
    }
}
```

### Integration Tests

```bash
# Run connection test
cargo test --test langfuse_minimal_test -- --nocapture

# Test with actual CLI
cargo build --bin chat_cli
./target/debug/chat_cli chat --trace --langfuse --no-interactive --trust-all-tools "test"
```

### Validate JSONL Output

```bash
# Check each line is valid JSON
jq empty ~/.q/traces/*.jsonl

# Pretty-print events
jq . ~/.q/traces/*.jsonl | less

# Count events by type
jq -r '.event_type' ~/.q/traces/*.jsonl | sort | uniq -c
```

---

## Debugging

### Enable Verbose Logging

```bash
export Q_LOG_LEVEL=trace
export RUST_LOG=chat_cli=trace

q chat "test"
tail -f ~/.q/logs/qchat.log
```

### Inspect Raw Stream Events

```bash
# Dump raw API responses
RUST_LOG=trace q chat "test" 2> raw_dump.log

# Search for event types
grep -i "ReasoningContentEvent\|ToolUseEvent" raw_dump.log
```

---

## Quick Win: CoT Capture in 30 Minutes

**File**: `crates/chat-cli/src/cli/chat/parser.rs`

**Add this match arm** (line ~420):

```rust
match output {
    ChatResponseStream::AssistantResponseEvent { content } => {
        // ... existing code
    },
    
    // ← ADD THIS BLOCK
    ChatResponseStream::ReasoningContentEvent(event) => {
        if let Some(cot_text) = event.text() {
            tracing::info!("🧠 CoT: {}", cot_text);
            // Later: trace_collector.log_agent_thought(cot_text);
        }
    },
    
    ChatResponseStream::ToolUseEvent { .. } => {
        // ... existing code
    },
    _ => {},
}
```

**Test**:
```bash
export Q_LOG_LEVEL=info
q chat "Explain how Rust ownership works"
tail -f ~/.q/logs/qchat.log | grep "🧠 CoT"
```

---

## Dependencies

**Already Present**:
```toml
✅ serde = { version = "1.0", features = ["derive"] }
✅ serde_json = "1.0"
✅ tokio = { version = "1", features = ["full"] }
✅ tracing = "0.1"
```

**Need to Add**:
```toml
uuid = { version = "1.0", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
```

---

## Known Issues

### 1. Langfuse Dashboard Display
**Status**: Events sent successfully (Status 201) but not appearing in dashboard  
**Evidence**: API accepts events, possible format mismatch  
**Tracking**: See `LANGFUSE_PROGRESS.md`

### 2. Chain of Thought Capture
**Status**: Not yet implemented  
**Reason**: ReasoningContentEvent exists but falls through to `_ => {}`  
**Solution**: Add match arm in parser.rs

### 3. OpenTelemetry Integration
**Status**: Planned  
**Goal**: Standard observability format for broader tool support

---

## Performance Considerations

### Async Buffering

```rust
pub struct TraceCollector {
    event_tx: mpsc::UnboundedSender<TraceEvent>,
    // Background task handles flushing to sinks
}
```

### Overhead Estimates

| Component | Overhead | Mitigation |
|-----------|----------|------------|
| Event serialization | <1ms | `serde_json` optimized |
| JSONL writes | <0.5ms | Async buffering |
| Langfuse batching | <10ms | Background task |
| Memory | ~5MB/session | Size limits |

**Total Impact**: <5% latency increase

---

## Security

### Already Handled
1. **File Permissions**: `logging.rs` sets 0o600 (owner-only)
2. **Log Rotation**: 10MB limit
3. **Sensitive Data**: Debug redaction in place

### New Concerns
1. **API Keys**: Redact from logs
2. **PII in Prompts**: User responsibility, optional redaction available
3. **Trace File Access**: Inherits log file security model

---

## Next Steps

### Immediate
1. ✅ Implement CoT capture (add match arm)
2. 🔄 Debug Langfuse dashboard display
3. ⏳ Add OpenTelemetry support

### Short Term
- Proper shutdown/flush handling
- Configurable batch sizes
- Retry logic improvements
- PII redaction

### Long Term
- Real-time streaming to Langfuse
- Local trace viewer (`q trace view`)
- Trace replay functionality
- ACE reflection loop

---

## References

- [User Guide](OBSERVABILITY.md) - Usage and troubleshooting
- [Roadmap](ROADMAP.md) - Project plan and timeline
- [Langfuse Progress](LANGFUSE_PROGRESS.md) - Current debugging status
- [Amazon Q Context](AmazonQ.md) - Q CLI context entry point
