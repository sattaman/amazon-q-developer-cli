# Amazon Q CLI Architecture & Trace Retrieval

do List Feature](docs/todo-lists.md) - Using Q's built-in task management

## Core Architecture

### Streaming Protocol

- Amazon Q uses a **streaming event-based protocol** (similar to Lex V2's `StartConversationResponseEventStream`)
- API likely: `codewhisperer:CreateTaskAssistConversation`
- Events delivered incrementally over HTTP/2 persistent connection
- Client abstracts intermediate events (CoT, tool proposals) and renders only final response

### Technology Stack

- **Language**: Rust (compiled binary for performance)
- **SDK**: AWS SDK for Rust
- **Logging**: Uses `tracing` crate (controlled via `RUST_LOG` env var)
- **Serialization**: `serde` + `serde_json`

## Internal Data Structures

### API Response Payload Contains

1. **Chain of Thought (CoT)**: Step-by-step reasoning (field like `thought_process` or `intermediate_steps`)
2. **Tool Proposals**: Structured objects with:
   - Tool name
   - Operation ID
   - Parameters (OpenAPI-style schema)
3. **Tool Results**: Raw output from executed tools
4. **Final Response**: User-facing text

### Evidence of Structured Payloads

- CLI supports `--trust-tools=fs_read,fs_write` (requires structured tool names)
- Permission prompts for tool execution (requires parsing tool proposals)
- `/reply` command for quoting specific agent points (requires structured turn tracking)

## Trace Retrieval Strategies

### Tier 1: Non-Invasive (RUST_LOG)

```bash
RUST_LOG=trace q chat "question" 2> raw_api_dump.log
```

**Pros**: No code changes  
**Cons**: Unstructured logs, heavy post-processing, fragile parsing

### Tier 2: Source Modification (Recommended)

Inject JSON emitter hook in stream handler:

1. Locate conversation stream processing loop
2. Identify event structs for CoT and tool calls
3. Serialize to JSONL: `serde_json::to_string(&event)`
4. Write to dedicated trace file

**Critical**: Client-side events (`/reply`, interrupts) never reach API—must log locally

## Key Integration Points

### 1. Stream Handler

- **Location**: Function calling `CreateTaskAssistConversation`
- **Hook**: Deserialize events, emit to JSONL before rendering

### 2. Input Parser

- **Location**: Terminal input loop handling `/reply` command
- **Hook**: Emit `user_interrupt` events with correction text

### 3. Tool Executor

- **Location**: Logic requesting user permission for tools
- **Hook**: Log tool lifecycle (propose → confirm → execute → result)

## ACE Schema Requirements (JSONL)

Required fields per line:

```json
{
  "trace_id": "uuid",
  "turn_index": 0,
  "timestamp_utc": "ISO 8601",
  "event_type": "user_prompt|agent_thought|tool_execute|user_interrupt",
  "user_input": "string",
  "agent_thought_trace": "CoT string",
  "tool_calls_executed": [{"name": "...", "params": {...}}],
  "tool_output": "string",
  "interrupt_flag": false,
  "final_response": "string"
}
```

## Constraints

### Non-Negotiable

- **Client-side only**: No AWS account access, no CloudWatch/S3
- **No server-side telemetry**: All logging must happen locally

### Maintenance Risks

- AWS SDK internal structs may change without notice
- Custom binary requires monitoring for SDK updates
- License compliance review required before distribution

## CLI Features Relevant to Tracing

- `-v, -vv, -vvv, -vvvv`: Verbosity levels (insufficient for structured data)
- `--trust-all-tools`: Global tool permission (bypasses prompts)
- `--trust-tools=<list>`: Selective tool trust
- `--no-interactive`: Non-interactive mode (essential for automated testing)
- `/reply`: Quote and respond to specific agent points (high-fidelity feedback)

### **IMPORTANT: Automated Testing Flags**

For automated testing and CI/CD, always use:

```bash
q chat --no-interactive --trust-all-tools --trace --langfuse "your question"
```

- `--no-interactive`: Prevents waiting for user input/confirmations
- `--trust-all-tools`: Auto-approves all tool executions
- `--trace`: Enables JSONL trace logging
- `--langfuse`: Enables Langfuse OpenTelemetry export

## Next Steps for Implementation

1. **Locate stream handler**: Search for `CreateTaskAssistConversation` or similar API call
2. **Identify event structs**: Find Rust types for `AgentThought`, `ToolProposal`, etc.
3. **Add JSONL writer**: Create file handle in `~/.q/traces/{trace_id}.jsonl`
4. **Hook serialization**: `writeln!(trace_file, "{}", serde_json::to_string(&event)?)`
5. **Test with RUST_LOG=trace**: Validate raw payload structure before modifying code
