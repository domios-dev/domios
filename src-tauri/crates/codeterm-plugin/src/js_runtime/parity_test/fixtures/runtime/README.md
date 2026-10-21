# Generic runtime hook fixtures

`shared.json` contains literal plans, terminal results/refusals and ordered reductions for a small provider implemented independently in native Rust and `plugin.js`. `CODETERM_GENERIC_RUNTIME_FIXTURES` overrides this directory by path. No real provider identity or protocol mapping appears in the generic harness.

Run the Node fixture oracle with:

```sh
node --test src-tauri/crates/codeterm-plugin/src/js_runtime/parity_test/fixtures/runtime/verify.test.cjs
```

Canonical Rust selectors (never compile in a Worker lane):

```sh
./build.sh test-one plugin js_runtime::parity_test::runtime::runtime_native_quickjs_shared_fixture_parity
./build.sh test-one plugin js_runtime::provider::runtime_hooks::tests::
./build.sh test-one plugin js_runtime::manifest::agent_runtime::tests::
./build.sh test-one plugin agent_runtime::authority::tests::
./build.sh test-one app provider::registry::tests::runtime_capability_selects_chat_transport_and_retains_cli_controls
```

The provider replay discovers fixture inputs from plugin bundle directories containing `tests/fixtures/runtime`, `tests/runtime-manifest.json` and `plugin.json`. It reads the eight shared scenarios and the optional `captured/` scenarios. It has no provider-specific path or identity. Three optional overrides select explicit paths: `CODETERM_RUNTIME_FIXTURES` (the eight-scenario directory), `CODETERM_RUNTIME_MANIFEST` (the plugin's live manifest), and `CODETERM_RUNTIME_DECLARATION` (its test-only runtime/chat capability declaration). The replay loads the manifest's entry bundle and overlays only those two capability declarations. It neither activates the provider nor starts its process.

```sh
./build.sh test-one plugin js_runtime::parity_test::runtime::replay::runtime_shared_provider_fixture_replay
```

The frame benchmark uses the same discovered or supplied paths and derives observed peak one-second and sustained ten-second notification rates from captured `receiptMs` timestamps. It requires a real complete ten-second capture, with no environment variable needed. It reports lazy runtime-context first-call latency, steady QuickJS frames per second and p99 per batch (batch size 1), and a generic native frame-projection baseline on the identical frames. The native baseline is a bridge-overhead comparator; it does not implement the real provider's protocol. QuickJS must exceed twice the observed streaming rate. It also reports the comparison with twice the peak rate. No rate is invented or silently defaulted.

```sh
./build.sh test-one plugin js_runtime::parity_test::runtime::replay::runtime_quickjs_frame_mapping_benchmark
```

Missing fixtures or a complete sustained capture, malformed fixture, uncorrelated reply, unexpected action or unfinished operation fails the gate. Hook state stays caller-owned and bounded. A local `questionAnswer` action produces only typed input and never creates an RPC request.
