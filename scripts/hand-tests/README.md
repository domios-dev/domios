# Hand Tests

Measure one native Windows resize transaction against the visible dev terminal:

```powershell
node scripts/hand-tests/terminal-native-resize.mjs
```

Measure event-driven agent startup for every registered PTY provider on dev:

```powershell
node scripts/hand-tests/agent-spawn-latency.mjs
```
