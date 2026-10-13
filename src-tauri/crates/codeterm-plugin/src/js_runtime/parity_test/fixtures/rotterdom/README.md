# Rotterdom decision parity fixture

This is the actual 0.2.0 bundle and manifest from `rollacode/codeterm-plugins`, commit `313479b10c858534f1ea98beed126cd78bdca717` (PR #30). Capability plugins release separately; this snapshot lets host parity run offline without another repository checkout. It is test data, not a bundled system plugin.

`plugin.js` SHA-256: `4d01f822e98ed97c6e44def744fa3e22f8f59c96eae351f4a936f46ee7e594b4`. `shared.json` is byte-identical to `rotterdom/fixtures/decision/shared.json`, used by the plugin's Node tests and the host's native/JS parity tests. When the adapter changes, rebuild Rotterdom and refresh these three files from its reviewed commit. Run `node rotterdom/plugin.test.cjs` in the plugin lane and `./build.sh nextest plugin 'test(/parity_test/)'` in canonical.
