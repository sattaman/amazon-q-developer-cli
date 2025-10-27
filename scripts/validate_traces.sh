#!/bin/bash
# Validate trace JSONL files against ACE schema

set -e

TRACE_DIR="${1:-$HOME/.q/traces}"

if [ ! -d "$TRACE_DIR" ]; then
    echo "No trace directory found at $TRACE_DIR"
    exit 0
fi

echo "Validating traces in $TRACE_DIR..."

for trace_file in "$TRACE_DIR"/*.jsonl; do
    if [ ! -f "$trace_file" ]; then
        continue
    fi
    
    echo "Checking $trace_file..."
    
    # Validate each line is valid JSON
    if ! jq empty "$trace_file" 2>/dev/null; then
        echo "ERROR: Invalid JSON in $trace_file"
        exit 1
    fi
    
    # Check required fields exist
    while IFS= read -r line; do
        if ! echo "$line" | jq -e '.trace_id and .turn_index != null and .timestamp_utc and .event_type' >/dev/null 2>&1; then
            echo "ERROR: Missing required fields in line: $line"
            exit 1
        fi
        
        # Validate event_type is one of the allowed values
        event_type=$(echo "$line" | jq -r '.event_type')
        case "$event_type" in
            user_prompt|agent_thought|tool_execute|tool_output|user_interrupt|final_response)
                ;;
            *)
                echo "ERROR: Invalid event_type: $event_type"
                exit 1
                ;;
        esac
    done < "$trace_file"
    
    echo "✓ $trace_file is valid"
done

echo "All traces validated successfully!"
