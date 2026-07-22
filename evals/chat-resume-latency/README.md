# Restored Chat UI latency bench

Measures the dev-only path from an HTTP send to two observable events:

- provider status changing to `Working`;
- a new assistant transcript delta containing a computed answer absent from the prompt.

The bench opens each pane's Chat UI tailer before sending and consumes the daemon SSE
stream. It uses bounded event waits, not sleeps or transcript polling.

```bash
bash evals/chat-resume-latency/run.sh
bash evals/chat-resume-latency/run.sh --targets Goya Hooke Tesla Kelvin
bash evals/chat-resume-latency/run.sh --self-test
```

Each probe reports Chat attachment, synchronous send return, provider Working, and
assistant reply latency. Missing replies fail the run after the deadline. The latest
JSON result is written to `target-check/evals/chat-resume-latency-latest.json`.

<!-- revision: 1 -->
