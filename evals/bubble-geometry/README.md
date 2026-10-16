# Bubble geometry

Read-only geometry oracle for the rendered DEV transcript and composer. No messages, sessions or daemon state are created or changed. Only the Browser viewport is temporarily overridden and cleared in a finally block.

Open the Domios Browser, navigate to the DEV frontend (the Vite port in DEV health), and select an existing loaded Chat containing user and assistant messages and a composer. Use the Browser snapshot to confirm the selected conversation. Obtain its endpoint with `codeterm plugin browser endpoint` outside the bench, then run:

```bash
bash evals/bubble-geometry/run.sh --cdp ws://127.0.0.1:PORT/devtools/browser/ID
```

Optional `--target ID` selects one DEV page when several are open. The runner uses the bench `dc` and `dev_run` wrappers, requires default DEV on 7686, and refuses unrelated Browser pages. It measures the actual production components, not a fixture or injected stylesheet. Run against the integrated SHA; the JSON records the observed daemon build. A daemon SHA alone does not establish the frontend source revision.

See [SCENARIOS.md](SCENARIOS.md). Exit 0 means all four sizes passed. Missing prerequisites, protocol errors, widths or alignment outside a 2px tolerance fail. Each scenario also runs a negative control against a copied measurement to prove that the oracle rejects an uncapped row. No CSS-class or inline-style assertions are added to unit tests.

The shared max is imported from `chatLayoutTokens.ts`; the in-page root font converts rem to pixels. Available width and center exclude transcript gutters and the scrollbar. User bubbles, assistant columns and transcript rows must fill that available width up to the token cap. Composer width must stay below the same cap.

History: `04b5fb56ac9df89f3f822556a5a405167e4c3901` replaced the timestamp-gutter rule with `.ct-chat-surface .ct-chat-message-row { max-width: none; }`, overriding the centered frame cap restored by `2ac53e7e6b0f072fec62070d65db891178eed849`. `dc5d9d258` changed only inset calculations; `a7fc13ffb` changed phone bubble tokens. Both intents are preserved.
