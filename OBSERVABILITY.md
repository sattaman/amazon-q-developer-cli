# Amazon Q CLI Observability

## Overview

Amazon Q CLI includes built-in observability features for capturing conversation traces with Langfuse integration.

## Quick Start

### Enable Tracing

```bash
# Local JSONL traces only
q chat --trace "Your question"

# With Langfuse integration
export LANGFUSE_SECRET_KEY=sk-lf-...
export LANGFUSE_PUBLIC_KEY=pk-lf-...
export LANGFUSE_HOST=http://localhost:3000

q chat --trace --langfuse "Your question"
```

### View Traces

```bash
# Local traces
cat ~/.q/traces/*.jsonl | jq .

# Langfuse dashboard
open http://localhost:3000
```

## What's Captured

- ✅ **User Prompts** - Questions asked
- ✅ **Tool Executions** - Tool calls with parameters
- ✅ **Tool Outputs** - Results from tools
- ✅ **Final Responses** - Assistant answers
- ✅ **User Interrupts** - `/reply` corrections
- ⏳ **Chain of Thought** - Coming soon (see TODO list ID: 1761584486065)

## Architecture

### Event Flow
```
User Input → TraceCollector → [JsonlSink, LangfuseSink] → Storage
```

### Key Components

- **TraceCollector** (`crates/chat-cli/src/observability/collector.rs`)
  - Manages trace lifecycle
  - Coordinates sinks
  
- **JsonlSink** (`crates/chat-cli/src/observability/sinks/jsonl.rs`)
  - Writes to `~/.q/traces/{trace_id}.jsonl`
  
- **LangfuseSink** (`crates/chat-cli/src/observability/sinks/langfuse.rs`)
  - Sends to Langfuse API
  - Batches events for efficiency

### Event Types

```rust
pub enum TraceEvent {
    UserPrompt { trace_id, turn_index, timestamp_utc, user_input },
    AgentThought { trace_id, turn_index, timestamp_utc, agent_thought_trace },
    ToolExecute { trace_id, turn_index, timestamp_utc, tool_calls_executed },
    ToolOutput { trace_id, turn_index, timestamp_utc, tool_use_id, tool_output },
    UserInterrupt { trace_id, turn_index, timestamp_utc, interrupt_flag, user_input },
    FinalResponse { trace_id, turn_index, timestamp_utc, final_response },
}
```

## Configuration

### CLI Flags

- `--trace` - Enable observability
- `--langfuse` - Enable Langfuse integration
- `--trace-dir <PATH>` - Custom output directory

### Environment Variables

- `LANGFUSE_SECRET_KEY` - API secret key
- `LANGFUSE_PUBLIC_KEY` - API public key
- `LANGFUSE_HOST` - Langfuse instance URL (default: https://cloud.langfuse.com)

## Implementation Status

### Completed
- ✅ Event structure with Langfuse envelope format
- ✅ JSONL local storage
- ✅ Langfuse API integration
- ✅ User prompt capture
- ✅ Tool execution capture
- ✅ Response capture
- ✅ Interrupt detection

### In Progress
- 🔄 Langfuse dashboard display (events sent but not showing)
- 🔄 Chain of Thought (CoT) capture

### Planned
- ⏳ Proper shutdown/flush handling
- ⏳ Configurable batch sizes
- ⏳ Retry logic improvements
- ⏳ PII redaction

## Troubleshooting

### Events not in Langfuse dashboard

**Symptoms**: API returns 201 but traces don't appear

**Debug**:
```bash
# Check local traces are created
ls -lht ~/.q/traces/ | head -5

# Run with debug output
RUST_LOG=info q chat --trace --langfuse "test"

# Verify environment variables
echo $LANGFUSE_HOST
```

**Common causes**:
- Event format mismatch
- Missing required fields in body
- Langfuse version compatibility

### No events captured

**Check**:
1. `--trace` flag is set
2. TraceCollector is initialized (look for "🔍 Observability enabled" in stderr)
3. Events are being emitted (check JSONL files)

## Development

### Adding New Event Types

1. Add variant to `TraceEvent` enum in `events.rs`
2. Add mapping in `LangfuseSink::map_event()`
3. Emit event at appropriate location in `mod.rs`

### Testing

```bash
# Run connection test
cargo test --test langfuse_minimal_test -- --nocapture

# Test with actual Q CLI
cargo build --bin chat_cli
./target/debug/chat_cli chat --trace --langfuse --no-interactive --trust-all-tools "test"
```

## References

- [Langfuse API Docs](https://langfuse.com/docs/tracing-data-model)
- [Implementation Plan](OBSERVABILITY_ACE_PLAN.md)
- [Integration Guide](CODEBASE_INTEGRATION_GUIDE.md)
- [Key Findings](KEY_FINDINGS.md)
- [Current Progress](LANGFUSE_PROGRESS.md)
