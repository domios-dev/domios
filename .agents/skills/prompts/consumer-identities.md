# Consumer identity inventory

Use this classification when maintaining procedures, SDK consumers and naming
guards. Inspect the executing source before changing any identifier.

| Surface | Current contract | Deliberate retained identity |
|---|---|---|
| Operator commands | Teach `domios`; `codeterm` is a fully working alias | Existing alias tests, installed alias bytes and allowlisted subprocess names |
| Host selectors | `CODETERM_*` and the `CODETERM` marker stay frozen | PTY exports in `codeterm-service-core/src/service/pty_io/tab_env.rs`, CLI clap env selectors and provider hooks; no `DOMIOS_*` replacement |
| Instance storage and sockets | `.domios`, `.domios-dev[-<instance>]`, `domios[-dev[-<instance>]]` socket prefix | `CODETERM_DATA_DIR`, `CODETERM_CONFIG_DIR` and `CODETERM_INSTANCE` remain the selectors; release ignores the named-dev selector |
| SDK home expansion | `~/.domios[/...]` resolves through `codeterm_paths::data_dir()` | Other `~/` paths, including `~/.codeterm`, expand as ordinary user-home paths; filesystem grants remain independent |
| Official external consumers | Aider startup/primer and Rotterdom history/CLI-bin paths use `~/.domios/` | Sources and independently versioned bundles live in `rollacode/codeterm-plugins`; a lane commit alone is not a marketplace release |
| Rust and npm identifiers | Keep current Cargo/package/import names | `codeterm-*`, `codeterm_*`, `@codeterm/*`, `minCodeterm`, `codeterm-core` and `codeterm-plugins` are technical identities; a product rename does not authorize renaming them |
| Skill and prompt identifiers | Keep loader and skill ids | `skills/domios`, `codeterm_cli.md`, `CODETERM_CLI` and `codeterm_rule.md` remain source identifiers |
| Repository and infrastructure paths | Use paths that exist in the checkout | `.codeterm/build-machine/`, `.codeterm/specs/`, upstream repository URLs, runner names, notary profiles and opaque cache/repair filenames are not the per-user data directory |
| Relaunch environment scrubbing | Remove inherited instance selectors and integration paths | Former `.codeterm` shell-integration paths and `TERM_PROGRAM=CodeTerm` remain rejection inputs; removing them would weaken isolation |
| Removed provider | No active install or spawn recipe for the retired plugin | `evals/watcher-health/README.md` records the retired oracle and missing live proof; it supplies no launch command |
| Endpoint connections | Discover actual installed plugin capabilities | Rotterdom supports OpenAI-compatible endpoints. LM Studio is an endpoint brand, including in `src/help/content/en.ts`; that brand does not establish watcher support |
| Captured evidence | Preserve bytes and original identifiers | `plugins/*/tests/fixtures/`, `evals/tab-state-jev/screens.jsonl`, its recorded measurements/E2E oracle, `captures/`, `evals/**/evidence/`, `docs/release/` and `.research/` retain historical names |
| Repository instructions | Follow the human edit boundary | `AGENTS.md` requires an explicit human request before edits; do not treat its historical paths as authority to change runtime contracts |

`scripts/rename-guard.mjs --scope identity` is a strict naming check;
`--scope docs --report` inventories occurrences and exits successfully even when
there are violations. Its occurrence counts include technical names and history;
neither report mode nor a source inventory proves live behavior. Maintain narrow
path/literal exceptions in `scripts/rename-guard-allowlist.json`, preserving
rejection-input coverage. The phase ownership manifest records the earlier broad
crate rename and is not the current task's edit grant.

For bridge changes, retain canonical instance-resolution coverage and an ordinary
home-path negative case. Official plugin checks must exercise the regenerated
bundles with mocks that accept only canonical instance prefixes. Publication
receipts must identify source SHA, plugin versions, remote default-branch SHA and
installed channel provenance. Native Windows/macOS/Linux observations are separate
from source review and compilation. The owner must publish reviewed consumer
releases before or with the app release that removes their old bridge.
