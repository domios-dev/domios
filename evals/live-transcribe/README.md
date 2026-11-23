# Live Transcribe concierge evaluation

Run from the repository root with Node.js 22 or newer. The harness uses synthetic transcripts, tab IDs, attachment references and memory facts. Route, document and memory writes become entries in a local ledger. Screenshots are represented by IDs and metadata. The only live connection is to the configured model endpoint.

## Run the fixtures

```sh
node --test evals/live-transcribe/protocol.test.mjs
node evals/live-transcribe/runner.mjs --mock-llm --mem-lookup ab --scenario all
```

`long-session` compresses 61 minutes of speech, pauses between turns, tab switches, screenshot drops and two restart checkpoints into immediate replay. `mem-enrichment` tests missing references against a fixed corpus. Mock latency and tokens are synthetic. The mock exercises the protocol and scoring with scripted decisions.

## Benchmark local candidates

Load a tool-calling candidate in LM Studio, start its local server, and substitute the server's exact model identifier for `YOUR_MODEL_ID`. Repeat for each candidate considered for `rotterdom`.

```sh
node evals/live-transcribe/runner.mjs --model "YOUR_MODEL_ID" --mem-lookup ab --scenario all
node evals/live-transcribe/runner.mjs --model "YOUR_MODEL_ID" --mem-lookup ab --scenario all --format json > live-transcribe-scorecard.json
```

The default endpoint is `http://127.0.0.1:1234/v1`. For another OpenAI-compatible endpoint, pass its API base URL. The adapter uses Chat Completions with native function tool calls. Supply the name of an existing environment variable with `--api-key-env` if the server requires a bearer key.

```sh
node evals/live-transcribe/runner.mjs --base-url http://127.0.0.1:1234/v1 --model "YOUR_MODEL_ID" --api-key-env LM_STUDIO_API_KEY --mem-lookup ab --scenario all
```

`--scenario long-session` and `--scenario mem-enrichment` isolate each fixture. `--mem-lookup off|on|ab` selects the tool set. `--max-chars 512` exercises pagination with a smaller Unicode scalar budget for the native text frame. Each page carries the cursor metadata in its `meta` field and the attachment references alongside the text. The cursor advances only through fully exposed rows. Transport errors, malformed tool calls and invalid tool batches are recorded per turn. The CLI exits with status 1 when every turn in every selected configuration errors.

## Read the scorecard

- **Ambient false positives** count ambient turns that produce a write. Target 0%. Check the error count and overall accuracy as well, since a rejected batch can leave the ledger empty.
- **Routing precision and recall** match the exact tab ID, the fixture's required intent terms and the expected attachment IDs. Each expected route matches at most once.
- **Hold, epoch and restart** measure pending-speech holds, the three follow-up turns at the 10-turn boundaries, and the committed-route and held-cursor restart checks. After simulated restart, committed action replay must leave the ledger unchanged.
- **Shape completeness** checks headings, paragraph presence, bullet counts and overlap with the fixture's key words. Inspect the recorded patches for prose quality and meaning.
- **Memory A/B** reports lookup-on minus lookup-off accuracy and latency on the same fixture probes. Accuracy is the share of turns whose route, cursor action, document patch and durable fact match the oracle. Each turn can read at most twice and return at most 1,500 UTF-8 bytes from the fixture corpus.
- **Latency and tokens** include follow-up requests after memory lookup. Token means use turns with complete provider usage. JSON includes request counts and usage coverage for each turn. A dash marks a metric with no observations.

## Freeze a winning protocol

Compare candidates on both fixtures and record the chosen model ID, model settings, lookup mode, page budget and the scorecard alongside the lane commit SHA. Pin the `SYSTEM_PROMPT` and the generated OpenAI function schemas from `protocol.mjs` at that SHA. This command prints their exact values with the fixture tab directory and lookup enabled.

```sh
git rev-parse HEAD
node --input-type=module -e "import { SYSTEM_PROMPT, toolSchemas } from './evals/live-transcribe/protocol.mjs'; import { loadScenario } from './evals/live-transcribe/runner.mjs'; const { tabs } = await loadScenario('long-session'); process.stdout.write(JSON.stringify({ prompt: SYSTEM_PROMPT, tools: toolSchemas(tabs, true) }, null, 2))"
```

Use the winning lookup setting in that export. For the Core handoff, give Relay the pinned prompt, tool definitions and fixtures. Bind route targets to the live authorized tab directory and have Core enforce sequence bounds, atomic writes and acknowledgements, replay protection, and lookup limits. Configure the selected model for `rotterdom` in the same integration and rerun the fixture acceptance checks against the Core adapter before enabling it.
