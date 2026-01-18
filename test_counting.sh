#!/bin/bash
# Simple test to verify key counting doesn't double-count

set -e

echo "Testing wl-actions key counting..."

# Run a simple command that types a known number of characters
# We'll use 'echo test' which is 9 keystrokes (e,c,h,o, ,t,e,s,t) + Enter = 10 keys
output=$(timeout 2 wl-actions sh -c 'echo test; sleep 0.5' 2>&1 | grep "Key presses:" || true)

if [ -z "$output" ]; then
    echo "ERROR: No output from wl-actions"
    exit 1
fi

# Extract the key count
key_count=$(echo "$output" | grep -oP 'Key presses: \K\d+')

echo "Key count: $key_count"

# We expect around 10 keys (echo test + enter)
# Allow some variation for shell setup
if [ "$key_count" -ge 8 ] && [ "$key_count" -le 15 ]; then
    echo "✓ Test passed: Key count is reasonable ($key_count keys)"
    exit 0
else
    echo "✗ Test failed: Expected 8-15 keys, got $key_count"
    exit 1
fi
