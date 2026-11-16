# Rotterdom decision parity fixture

This is the actual 0.2.1 bundle and manifest from `domios-dev/domios-plugins`, commit `11b616a5fed7e1fb79eb831d8d9bbaa424e734fe`. Capability plugins release separately; this snapshot lets host parity run offline without another repository checkout. It is test data, not a bundled system plugin.

`plugin.js` SHA-256: `11001a1e09cc8d0628ccb6f2b7597d7eb18813fb8b68a7ac7d979c397bdced9c`. `shared.json` is byte-identical to `rotterdom/fixtures/decision/shared.json`, used by the plugin's Node tests and the host's native/JS parity tests. When the adapter changes, rebuild Rotterdom and refresh these three files from its reviewed commit. Run `node rotterdom/plugin.test.cjs` in the plugin lane and `./build.sh nextest plugin 'test(/parity_test/)'` in canonical.

Run `node --test src-tauri/crates/codeterm-plugin/src/js_runtime/parity_test/fixtures/rotterdom/verify.test.cjs` from the host checkout to exercise the exact parity transport setup and pinned bundle without Rust compilation. It checks requests with recursively sorted object keys, as produced by the host JSON bridge, and rejects changed instructions, criteria, state and score-level order.
