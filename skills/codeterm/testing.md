# CodeTerm Testing Guide

## How to launch and test CodeTerm with Claude inside

### 1. Launch app
```bash
# Dev mode
cd C:/Developer/codeterm && npx tauri dev

# Or release
RUST_LOG=info C:/Developer/codeterm/src-tauri/target/release/codeterm.exe
```

### 2. Wait for API ready
```bash
API="http://localhost:7685/api"
for i in $(seq 1 30); do
    P=$(curl -s $API/panes 2>/dev/null | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
    [ -n "$P" ] && break; sleep 2
done
```

### 3. Launch Claude with skip permissions
```bash
curl -s -X POST "$API/panes/$P/send" -H "Content-Type: application/json" \
  -d '{"text":"claude --dangerously-skip-permissions\r\n"}' > /dev/null
```

### 4. Wait 300ms then press Enter for trust prompt
```bash
sleep 0.3
curl -s -X POST "$API/panes/$P/send" -H "Content-Type: application/json" \
  -d '{"text":"\r"}' > /dev/null
```

### 5. Wait for Claude to be FULLY ready
Check for "effort" marker (works in both normal and skip-permissions mode):
```bash
for i in $(seq 1 20); do
    sleep 2
    curl -s "$API/panes/$P/output" | grep -q "effort" && echo "Claude ready" && break
done
```
NOTE: "shortcuts" marker doesn't appear in `--dangerously-skip-permissions` mode!
Use "effort" or "bypass" as ready marker instead.

### 5.5 IMPORTANT: Wait for PS prompt BEFORE sending claude command
The PTY reader needs time to start. Send claude command only after PS prompt appears:
```bash
for i in $(seq 1 15); do
    sleep 2
    curl -s "$API/panes/$P/output" | grep -q "PS C:" && break
done
# NOW send claude
```

### 6. VERIFY Claude loaded before sending messages
```bash
curl -s "$API/panes/$P/output" | grep -q "Claude Code" && echo "VERIFIED" || echo "NOT READY"
```

### 7. Send natural language messages
```bash
curl -s -X POST "$API/panes/$P/send" -H "Content-Type: application/json" \
  -d '{"text":"split this terminal to the right\r"}' > /dev/null
```

### 8. Poll for result (every 2s)
```bash
for i in $(seq 1 20); do
    sleep 2
    # Check condition
    PC=$(curl -s $API/panes | grep -o '"id"' | wc -l)
    [ "$PC" -gt 1 ] && echo "Done!" && break
done
```

## Key points
- Always use `--dangerously-skip-permissions` to avoid approval prompts
- Wait for "shortcuts" text in output = Claude fully loaded
- Send `\r` (not `\r\n`) after Claude messages
- Poll every 2s, don't use fixed sleep
- Use natural language, not exact CLI commands
- Pipe stays alive with polling reader (no background thread)
