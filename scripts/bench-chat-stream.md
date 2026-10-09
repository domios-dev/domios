# Chat streaming regression benchmark

Run from the checkout with its own installed dependencies and Playwright Chromium:

```powershell
node scripts/bench-chat-stream.mjs C:/path/to/metrics.json
node scripts/bench-chat-stream.mjs C:/path/to/burst.json --burst
```

This launches a private headless browser and loopback server; it does not attach
to DEV or production. The default feeds a 600-line, 44,784-character answer in
64-character chunks on animation frames. `--burst` feeds 1,024-character chunks
on MessageChannel tasks. Both use the shared transcript store, selection index,
message parts context and MessageBubble renderer.

The benchmark requires exact stored and visible streaming text, settled markdown,
no long task above 200 ms through streaming and settling, and no CDP ping above 1,000 ms. It has a
60-second failure deadline. Output includes render count, streaming elapsed time,
long tasks, CDP ping maximum and the separate final markdown settle cost. Plain
text remains visible while React prepares the settled markdown in background
renders. The output path is optional; when supplied, a CPU profile is
saved beside it as `<path>.cpuprofile`.

`--baseline` records a failing performance baseline without enforcing performance
thresholds. Text integrity checks remain enforced.
