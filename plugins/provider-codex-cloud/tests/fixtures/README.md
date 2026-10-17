The task detail JSON files are unchanged fixtures from OpenAI Codex tag `rust-v0.162.1`, `codex-rs/backend-client/tests/fixtures/`:

- https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/backend-client/tests/fixtures/task_details_with_diff.json
- https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/backend-client/tests/fixtures/task_details_with_error.json

Tests add status-display fields using the installed client's `map_status` contract. Those status wrappers and the deterministic transport are modeled test inputs, not captured owner-account executions. The create request follows `cloud-tasks-client/src/http.rs` (`Tasks::create`) and the create acknowledgement follows `backend-client/src/client.rs` (`create_task`). No credential or account data is included.
