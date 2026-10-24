The opaque `runtime-missing-rollout.json` regression fixture wraps the reported
Codex 0.163.0 message in a generic invalid-request envelope (-32600).
[Upstream's zero-turn resume report](https://github.com/openai/codex/issues/42099)
documents that code and message shape. It is not a live capture. The generated
`runtime-rpc-error-schema-0.164.0.json` was captured with installed
`codex-cli 0.164.0` using `codex app-server generate-json-schema --out <pad>`.
It declares an integer code, a string message and untyped data; no typed
missing-rollout variant is declared for RPC replies. Turn error notifications
have a separate typed schema and do not classify thread/resume failures.

The provider fallback accepts only the exact bound thread's resume-stage reply
with code -32600 and absent/null data. Unknown typed data, other codes, other
threads and later verification failures retain ordinary refusal behavior.
Core receives only `thread_not_found`, and never parses vendor messages.
