# Langfuse Integration Progress - 2025-10-27

## ✅ Completed

### 1. Fixed Event Structure
- **Problem**: Events were missing the envelope wrapper required by Langfuse API
- **Solution**: Changed from complex enum to simple struct with `body: serde_json::Value`
- **File**: `crates/chat-cli/src/observability/sinks/langfuse.rs`
- **Result**: API now accepts events (Status 207/201)

### 2. Connection Verified
- **Test**: `crates/chat-cli/tests/langfuse_minimal_test.rs`
- **Result**: ✅ Connection works, traces created successfully

### 3. Event Capture Working
- **Events captured locally**: ✅ JSONL files in `~/.q/traces/`
- **Events being sent**: ✅ Logs show "Flushed X events to Langfuse: {successes:...}"
- **API Response**: ✅ Status 201 (created)

### 4. Current Event Coverage
- ✅ UserPrompt (questions)
- ✅ ToolExecute (tool calls with params)
- ✅ ToolOutput (tool results)
- ✅ FinalResponse (answers)
- ✅ UserInterrupt (/reply corrections)
- ❌ AgentThought (CoT) - **NOT YET IMPLEMENTED**

## 🔍 Current Issue

**Events are being sent successfully but not appearing in Langfuse dashboard**

### Evidence
```
✅ Flushed 1 events to Langfuse: {"successes":[{"id":"9e111be6-55d3-4483-a24b-a2303eefc110","status":201}],"errors":[]}
```

### Possible Causes
1. **Event format issue** - Events accepted by API but not displayed
2. **Trace ID format** - May need to be in specific format
3. **Missing required fields** - Body might be missing mandatory fields
4. **Langfuse version** - Local instance might need specific event structure

## 📋 Todo List Created

**ID**: 1761584486065 - "Add Chain of Thought (CoT) capture"

Tasks:
1. Locate parser event loop (line 410-420)
2. Add ReasoningContentEvent match arm
3. Extract reasoning text
4. Accumulate chunks
5. Emit AgentThought events
6. Add logging
7. Test with complex question
8. Verify in dashboard

## 🔧 Debug Changes Added

### Files Modified
1. `crates/chat-cli/src/observability/sinks/langfuse.rs`
   - Added eprintln debug logging
   - Changed BATCH_SIZE from 15 to 1 (immediate flush)
   - Added detailed flush logging

2. `crates/chat-cli/src/observability/collector.rs`
   - Added eprintln for sink creation
   - Added Drop implementation

## 🧪 Test Commands

### Run with full debug output:
```bash
cd /Users/thomas.sanderson/Documents/amazon-q-cli

# Set environment variables
export LANGFUSE_SECRET_KEY=sk-lf-355fb37d-0f10-4e8f-88eb-ab7bfe47baeb
export LANGFUSE_PUBLIC_KEY=pk-lf-90d38562-6e32-4b81-92cb-ff01dc8b99d8
export LANGFUSE_HOST=http://localhost:3000

# Build and run
cargo build --bin chat_cli --quiet
./target/debug/chat_cli chat --trace --langfuse --no-interactive --trust-all-tools "test question"
```

### Check local traces:
```bash
ls -lht ~/.q/traces/ | head -5
cat ~/.q/traces/$(ls -t ~/.q/traces/ | head -1) | jq .
```

### Run connection test:
```bash
cargo test --test langfuse_minimal_test -- --nocapture
```

## 🔍 Next Steps (Tomorrow)

### 1. Debug Event Format
- Compare our event structure with Langfuse docs
- Check if `traceId` field is required in body
- Verify timestamp format (ISO 8601)
- Check if we need to create trace first before spans

### 2. Test with Langfuse API Directly
```bash
# Test trace creation
curl -X POST http://localhost:3000/api/public/ingestion \
  -u "pk-lf-90d38562-6e32-4b81-92cb-ff01dc8b99d8:sk-lf-355fb37d-0f10-4e8f-88eb-ab7bfe47baeb" \
  -H "Content-Type: application/json" \
  -d '{
    "batch": [{
      "id": "test-envelope-id",
      "timestamp": "2025-10-27T17:00:00.000Z",
      "type": "trace-create",
      "body": {
        "id": "test-trace-id",
        "name": "test_trace"
      }
    }]
  }'
```

### 3. Check Langfuse Logs
```bash
# If running Langfuse in Docker
docker logs langfuse-container-name | grep -i error
```

### 4. Use Langfuse MCP Server
Ask Q to use the Langfuse MCP server to get correct event format:
```bash
q chat "Use the Langfuse MCP server to show me the correct format for creating a trace with spans"
```

### 5. Implement CoT Capture
Once events are showing up, implement the todo list for CoT capture.

## 📁 Key Files

- `crates/chat-cli/src/observability/sinks/langfuse.rs` - Langfuse sink implementation
- `crates/chat-cli/src/observability/collector.rs` - Trace collector
- `crates/chat-cli/src/observability/events.rs` - Event definitions
- `crates/chat-cli/src/cli/chat/mod.rs` - Event emission points
- `crates/chat-cli/tests/langfuse_minimal_test.rs` - Connection test
- `.env` - Environment variables

## 🌐 URLs

- Langfuse Dashboard: http://localhost:3000
- Langfuse API: http://localhost:3000/api/public/ingestion
- Langfuse Docs: https://langfuse.com/docs/tracing-data-model

## 📊 Current State

```
Local JSONL: ✅ Working
API Calls:   ✅ Successful (201)
Dashboard:   ❌ Not showing traces
CoT Capture: ❌ Not implemented
```
