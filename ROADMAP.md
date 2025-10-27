# Amazon Q CLI Observability - Roadmap

## Current Status

### ✅ Completed (Phase 1-3)
- Core trace infrastructure (TraceCollector, events, sinks)
- JSONL local storage with rotation
- Langfuse API integration (events sending)
- User prompt capture
- Tool execution capture
- Response capture
- Interrupt detection (`/reply`)
- Configuration via CLI flags and env vars
- Integration tests
- Documentation

### 🔄 In Progress
- **Chain of Thought (CoT) Capture**: ReasoningContentEvent exists but not captured
- **Langfuse Dashboard Display**: Events sent (Status 201) but not appearing in UI
- **OpenTelemetry Integration**: Planning phase

### ⏳ Planned
- Proper shutdown/flush handling
- Configurable batch sizes
- Enhanced retry logic
- PII redaction
- Performance optimization

---

## Phase 4: Chain of Thought Capture

### Goal
Capture agent reasoning from `ReasoningContentEvent` stream events.

### Tasks
- [ ] Add match arm in `parser.rs:420` for `ReasoningContentEvent`
- [ ] Extract reasoning text from event
- [ ] Accumulate CoT chunks (if streaming)
- [ ] Emit `AgentThought` events to TraceCollector
- [ ] Add debug logging for CoT capture
- [ ] Test with complex reasoning questions
- [ ] Verify in Langfuse dashboard

### Files to Modify
- `crates/chat-cli/src/cli/chat/parser.rs` (~10 lines)
- `crates/chat-cli/src/observability/collector.rs` (add log_agent_thought method)

### Estimated Time
4-8 hours

---

## Phase 5: Langfuse Dashboard Debugging

### Goal
Fix event display in Langfuse UI (events currently accepted but not shown).

### Investigation Tasks
- [ ] Compare event format with Langfuse API docs
- [ ] Test with minimal curl request to API
- [ ] Check if `traceId` field required in body
- [ ] Verify timestamp format (ISO 8601)
- [ ] Test creating trace before spans
- [ ] Check Langfuse server logs for errors
- [ ] Use Langfuse MCP server for correct format

### Possible Issues
1. Event format mismatch (missing required fields)
2. Trace ID format incorrect
3. Need to create trace explicitly before spans
4. Langfuse version compatibility

### Estimated Time
8-16 hours

---

## Phase 6: OpenTelemetry Integration

### Goal
Add OpenTelemetry support for broader observability tool compatibility.

### Requirements
- Standard OTLP format
- Span/trace model mapping
- Exporter configuration
- Support for Jaeger, Zipkin, etc.

### Tasks
- [ ] Research OpenTelemetry Rust SDK
- [ ] Design TraceEvent → OTLP mapping
- [ ] Implement OTLP sink
- [ ] Add configuration for OTLP endpoint
- [ ] Test with Jaeger backend
- [ ] Document setup for various backends

### Files to Create
- `crates/chat-cli/src/observability/sinks/otlp.rs`
- `docs/opentelemetry-setup.md`

### Dependencies
```toml
opentelemetry = "0.21"
opentelemetry-otlp = "0.14"
```

### Estimated Time
2-3 weeks

---

## Phase 7: Production Hardening

### Goal
Make observability production-ready with proper error handling and performance.

### Tasks

#### Shutdown Handling
- [ ] Implement graceful shutdown for TraceCollector
- [ ] Flush all pending events on exit
- [ ] Add timeout for flush operations
- [ ] Handle Ctrl+C gracefully

#### Configuration
- [ ] Make batch sizes configurable
- [ ] Add sampling rate support
- [ ] Add sink enable/disable per sink
- [ ] Support config file (`~/.q/config.toml`)

#### Retry Logic
- [ ] Exponential backoff for Langfuse
- [ ] Max retry attempts configuration
- [ ] Dead letter queue for failed events
- [ ] Circuit breaker pattern

#### Performance
- [ ] Benchmark overhead with/without tracing
- [ ] Optimize serialization hot paths
- [ ] Add memory limits for event buffer
- [ ] Profile and optimize critical paths

### Estimated Time
3-4 weeks

---

## Phase 8: Advanced Features

### PII Redaction
- [ ] Regex-based redaction for emails, phones, API keys
- [ ] Configurable redaction rules
- [ ] Opt-in/opt-out per field
- [ ] Audit log for redactions

### Local Trace Viewer
- [ ] Terminal UI for browsing traces
- [ ] `q trace view <trace-id>` command
- [ ] Filter by event type
- [ ] Search within traces

### Trace Replay
- [ ] `q trace replay <trace-id>` command
- [ ] Re-execute conversation from trace
- [ ] Useful for debugging and testing
- [ ] Compare original vs replay results

### ACE Reflection Loop
- [ ] Analyze `interrupt_flag=true` events
- [ ] Identify context gaps in CoT
- [ ] Generate improved prompts/strategies
- [ ] Close self-improvement loop

### Estimated Time
4-6 weeks

---

## Timeline Summary

| Phase | Status | Duration | Key Deliverable |
|-------|--------|----------|-----------------|
| 1-3: Foundation | ✅ Complete | 4 weeks | Basic tracing working |
| 4: CoT Capture | 🔄 In Progress | 1 week | Reasoning visibility |
| 5: Langfuse Debug | 🔄 In Progress | 1 week | Dashboard working |
| 6: OpenTelemetry | ⏳ Planned | 3 weeks | OTLP support |
| 7: Hardening | ⏳ Planned | 4 weeks | Production-ready |
| 8: Advanced | ⏳ Planned | 6 weeks | Full feature set |

**Total Remaining**: ~15 weeks

---

## Success Criteria

### Functional
- ✅ All events from ACE schema captured
- ✅ JSONL output valid and parseable
- 🔄 Langfuse integration displays traces correctly
- ✅ < 5% performance overhead
- ✅ Zero data loss under normal operation
- ⏳ OpenTelemetry support for standard tools

### Quality
- ✅ 90%+ test coverage for observability code
- ✅ All integration tests pass
- ✅ Schema validation passes on 100% of traces
- ✅ No regressions in existing CLI functionality

### Usability
- ✅ Simple opt-in/opt-out via CLI flag or config
- ✅ Clear documentation with examples
- ✅ Helpful error messages if misconfigured
- ⏳ Multiple backend support (Langfuse, OTLP, local)

---

## Risk Mitigation

| Risk | Impact | Status | Mitigation |
|------|--------|--------|------------|
| CoT not accessible | High | ✅ Resolved | Found in ReasoningContentEvent |
| Langfuse API changes | Medium | 🔄 Monitoring | Version pinning, abstract interface |
| Performance degradation | High | ✅ Mitigated | Async processing, <5% overhead |
| Schema drift | Medium | ✅ Mitigated | Automated validation in CI |
| PII leakage | High | ⏳ Planned | Built-in redaction, documentation |
| OpenTelemetry complexity | Medium | ⏳ Planning | Start with basic OTLP, iterate |

---

## Questions & Decisions Needed

### Open Questions
1. **OpenTelemetry Priority**: Should this be Phase 6 or later?
2. **Langfuse vs OTLP**: Primary backend or equal support?
3. **PII Redaction**: Opt-in or opt-out by default?
4. **Sampling**: Needed for production or defer?

### Decisions Made
- ✅ Use Langfuse envelope format for events
- ✅ JSONL as primary local format
- ✅ Async processing for minimal overhead
- ✅ CLI flags for configuration (not just env vars)

---

## Next Actions

### This Week
1. 🎯 Complete CoT capture implementation
2. 🎯 Debug Langfuse dashboard display
3. 📝 Update documentation with CoT examples

### Next Week
4. 🎯 Start OpenTelemetry research
5. 🎯 Design OTLP sink architecture
6. 🧪 Add CoT integration tests

### This Month
7. 🎯 Complete OpenTelemetry basic support
8. 🎯 Implement graceful shutdown
9. 🎯 Add configurable batch sizes
10. 📊 Performance benchmarking

---

## Resources

- [Developer Guide](DEVELOPER_GUIDE.md) - Implementation details
- [User Guide](OBSERVABILITY.md) - Usage and troubleshooting
- [Langfuse Progress](LANGFUSE_PROGRESS.md) - Current debugging status
- [Amazon Q Context](AmazonQ.md) - Q CLI context entry point
- [Langfuse API Docs](https://langfuse.com/docs/tracing-data-model)
- [OpenTelemetry Rust](https://github.com/open-telemetry/opentelemetry-rust)
