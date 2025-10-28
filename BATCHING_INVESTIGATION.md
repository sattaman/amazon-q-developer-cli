# Langfuse Batching Investigation

## Problem Statement

Events are successfully sent to Langfuse with `BATCH_SIZE=1` (immediate send), but fail to appear in the dashboard when `BATCH_SIZE > 1` (batched sending).

## What Works

### BATCH_SIZE = 1 (Current Solution)
```rust
const BATCH_SIZE: usize = 1;  // Send immediately
```

**Behavior:**
- Each event triggers immediate HTTP POST to Langfuse
- Events appear in dashboard reliably
- trace-create → sent immediately
- generation-create → sent immediately

**Evidence:**
```bash
# Local trace shows both events captured
{"event_type":"user_prompt", "user_input":"hello"}
{"event_type":"final_response", "final_response":"Hi! I'm Amazon Q..."}

# Langfuse dashboard shows:
✅ Trace: "q-chat-session" with input "hello"
✅ Generation: "assistant_response" with output
```

## What Fails

### BATCH_SIZE = 2 or Higher
```rust
const BATCH_SIZE: usize = 2;  // Wait for 2 events before sending
const BATCH_SIZE: usize = 10; // Wait for 10 events before sending
```

**Behavior:**
- Events accumulate in batch
- Batch flushes when size reached OR timer fires (5s)
- trace-create event appears in dashboard
- generation-create event does NOT appear

**Evidence:**
```bash
# Local trace shows both events captured
{"event_type":"user_prompt"}
{"event_type":"final_response"}

# Langfuse dashboard shows:
✅ Trace: "q-chat-session" with input
❌ Generation: NOT visible (missing)
```

## Attempted Fixes

### Attempt 1: Flush on FinalResponse Event
**Code:**
```rust
// In collector.rs
if matches!(event, TraceEvent::FinalResponse { .. }) {
    lf.flush().await;
}
```

**Result:** ❌ Failed
- Flush was called but events still didn't appear
- No error messages
- Debug logging showed flush was being called

### Attempt 2: Explicit Flush on Exit
**Code:**
```rust
// In mod.rs ChatState::Exit handler
ChatState::Exit => {
    if let Some(ref collector) = self.trace_collector {
        collector.flush().await;
    }
    return Ok(());
}
```

**Result:** ❌ Failed
- Added flush call at program exit
- Still no events appearing with BATCH_SIZE > 1

### Attempt 3: Oneshot Channel for Flush Acknowledgment
**Code:**
```rust
pub async fn flush(&self) {
    let (tx, rx) = oneshot::channel();
    if self.flush_tx.send(tx).is_ok() {
        let _ = rx.await;  // Wait for actual completion
    }
}

// In batch_worker
Some(ack) = flush_rx.recv() => {
    if !batch.is_empty() {
        Self::flush_batch(...).await;
    }
    let _ = ack.send(());  // Signal completion
}
```

**Result:** ❌ Failed with BATCH_SIZE > 1
- Properly waits for flush to complete
- HTTP request should finish before exit
- But events still don't appear in dashboard

### Attempt 4: Reduce to BATCH_SIZE=1
**Code:**
```rust
const BATCH_SIZE: usize = 1;
```

**Result:** ✅ SUCCESS
- Every event sends immediately
- All events appear in dashboard
- Reliable and consistent

## Hypotheses for Why Batching Fails

### Hypothesis 1: Event Ordering in Batch
**Theory:** Langfuse requires events in specific order within a batch
- trace-create MUST come before any observations
- If events arrive out of order in batch, later events are rejected

**Test:** Send both events in single batch with correct order
```json
{
  "batch": [
    {"type": "trace-create", "body": {"id": "trace-123", ...}},
    {"type": "generation-create", "body": {"traceId": "trace-123", ...}}
  ]
}
```

**Status:** Not tested yet

### Hypothesis 2: Async Timing Issue
**Theory:** With batching, the HTTP request is in-flight when program exits
- Even with oneshot acknowledgment, tokio runtime might shut down
- HTTP request gets cancelled mid-flight
- Langfuse receives partial data

**Test:** Add longer sleep after flush acknowledgment
```rust
let _ = rx.await;
tokio::time::sleep(Duration::from_secs(2)).await;  // Extra safety margin
```

**Status:** Not tested yet

### Hypothesis 3: Langfuse API Batch Processing
**Theory:** Langfuse API has issues processing batches with multiple event types
- Single events work fine
- Batches with mixed types (trace-create + generation-create) fail silently
- API returns 201 but doesn't actually process all events

**Test:** Check Langfuse server logs for errors
```bash
docker logs langfuse-container | grep -i error
```

**Status:** Not tested yet

### Hypothesis 4: Event Deduplication
**Theory:** Langfuse uses envelope ID for deduplication
- With batching, events might have timing issues
- Duplicate envelope IDs cause rejection
- Only first event in batch is processed

**Test:** Verify envelope IDs are unique
```rust
eprintln!("Envelope ID: {}", envelope_id);
```

**Status:** Not tested yet

### Hypothesis 5: Race Condition in Batch Worker
**Theory:** The batch worker task is dropped before flush completes
- Even with oneshot, the background task might be cancelled
- Need to ensure task stays alive until HTTP completes

**Test:** Add logging at each step of flush
```rust
eprintln!("1. Flush signal received");
eprintln!("2. Calling flush_batch");
// ... after flush_batch
eprintln!("3. Flush complete, sending ack");
```

**Status:** Not tested yet

## Current Architecture

### Event Flow with BATCH_SIZE=1
```
UserPrompt event
  ↓
map_event() → trace-create
  ↓
batch.push() → batch.len() == 1
  ↓
flush_batch() → HTTP POST → 201 OK
  ↓
Langfuse dashboard ✅

FinalResponse event
  ↓
map_event() → generation-create
  ↓
batch.push() → batch.len() == 1
  ↓
flush_batch() → HTTP POST → 201 OK
  ↓
Langfuse dashboard ✅
```

### Event Flow with BATCH_SIZE=2 (BROKEN)
```
UserPrompt event
  ↓
map_event() → trace-create
  ↓
batch.push() → batch.len() == 1 (waiting...)

FinalResponse event
  ↓
map_event() → generation-create
  ↓
batch.push() → batch.len() == 2
  ↓
flush_batch() → HTTP POST → 201 OK
  ↓
Langfuse dashboard: ✅ trace, ❌ generation (WHY?)
```

## Key Questions to Answer

1. **Is the batch actually being sent?**
   - Check HTTP request logs
   - Verify payload contains both events

2. **Does Langfuse receive both events?**
   - Check Langfuse server logs
   - Look for ingestion errors

3. **Are both events in the same HTTP request?**
   - Log the payload before sending
   - Verify JSON structure

4. **Does Langfuse API have batch size limits?**
   - Check API docs for max events per batch
   - Test with different batch sizes (2, 5, 10)

5. **Is there a timing issue with the HTTP client?**
   - Try different HTTP timeout values
   - Test with synchronous HTTP client

## Debugging Steps

### Step 1: Log the Actual Payload
```rust
// In flush_batch, before sending
let payload_json = serde_json::to_string_pretty(&payload)?;
eprintln!("📤 Sending batch:\n{}", payload_json);
std::fs::write("/tmp/langfuse_batch.json", &payload_json)?;
```

### Step 2: Check Langfuse Response
```rust
// After HTTP request
let response_body = resp.text().await?;
eprintln!("📥 Langfuse response: {}", response_body);
```

### Step 3: Test with Curl
```bash
# Send the exact same batch with curl
curl -X POST http://localhost:3000/api/public/ingestion \
  -u "pk-lf-...:sk-lf-..." \
  -H "Content-Type: application/json" \
  -d @/tmp/langfuse_batch.json \
  -v
```

### Step 4: Check Langfuse Server Logs
```bash
# If running in Docker
docker logs langfuse-server 2>&1 | grep -i "error\|ingestion"

# Check for specific trace ID
docker logs langfuse-server 2>&1 | grep "trace-123"
```

### Step 5: Test Event Order
```rust
// Ensure trace-create comes before generation-create in batch
// Current code should already do this, but verify
```

## Research Questions

1. **Langfuse API Documentation:**
   - Does the ingestion API have specific requirements for batch ordering?
   - Are there limits on batch size or event types per batch?
   - Does the API process events sequentially or in parallel?

2. **Tokio Runtime Behavior:**
   - When does tokio shut down background tasks?
   - Does `oneshot::channel().await` guarantee task completion?
   - Could the HTTP request be cancelled even with await?

3. **Reqwest Client:**
   - Does reqwest properly handle connection pooling with batches?
   - Could there be a timeout issue with larger payloads?
   - Is there a difference between sending 2 requests vs 1 batch?

## Workaround (Current)

**Use BATCH_SIZE=1** until batching issue is resolved.

**Pros:**
- ✅ Reliable - all events appear
- ✅ Simple - no complex flush logic needed
- ✅ Immediate - events appear in dashboard instantly

**Cons:**
- ❌ More HTTP requests (2-5 per conversation)
- ❌ Higher latency overhead
- ❌ Not scalable for high-volume scenarios

## Next Steps

1. Add payload logging to see exact JSON being sent
2. Compare single-event payload vs batched payload
3. Test with curl to isolate client vs server issue
4. Check Langfuse server logs for processing errors
5. Research Langfuse API batch processing behavior

## Files to Check

- `crates/chat-cli/src/observability/sinks/langfuse.rs` - Batch worker logic
- `crates/chat-cli/src/observability/collector.rs` - Event emission
- Langfuse server logs - Processing errors
- Network logs - HTTP request/response details
