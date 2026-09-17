# Code style: SOLID, DRY, no comments

**SOLID + DRY are non-negotiable.** Single responsibility per module, dependencies flow through explicit interfaces, no repeated logic. Duplicated 20-line blocks across three files is a refactor signal, not a copy-paste signal.

## Comments — default no

**Code is prose — good code reads itself.** This rule is non-negotiable and Claude has been a chronic offender. Read this twice before writing any comment:

- **Default: no comment.** Names, types, and structure carry the meaning. Documentation lives in `docs/`, not inline.
- **Never document WHAT the code does.** If you feel that urge, the code is unclear — rename a variable, extract a helper, split a function.
- **Never reference the current task, fix, or caller.** No "Phase 3 adds X", no "used by Y", no "fixes issue Z". That belongs in commit messages and PR descriptions. Comments rot; commit history doesn't.
- **Never write multi-line comment blocks.** One line is the max for anything that isn't a full external-reference URL. If you need more than one line, you're documenting WHAT (delete it) or you're explaining bad code (fix the code).
- **Never write `///` field docs for self-describing field names.** `workspace_id: Option<String>` does not need `/// The workspace id.`
- **Never write module-level `//!` doc blocks that describe the module's purpose.** The module name does that.
- **Never write JSX label comments** like `{/* Header */}` / `{/* Agent provider + model */}`. The JSX below already names itself.
- **Never write step-numbered narration** like `// 1. Resolve workspace`. The function body is linear and the code is the comment.

Write a comment only when all of these are true:
1. The code itself CANNOT be made clearer by renaming or extracting.
2. The reader would otherwise have to guess WHY a line exists (lock ordering, a specific OS quirk, an SDK bug you're working around, a non-obvious invariant).
3. One line is enough to capture that WHY.

Everything else gets deleted. If you catch yourself writing a comment and you can't point to a clear WHY in under 15 words, the comment doesn't belong there. Pattern-copy discipline matters too: just because the file around you already has bloat doesn't give you license to add more — fix the surrounding bloat first if it's distracting you into matching the style.

## Rust specifics

- `parking_lot::Mutex` for shared state. No `tokio::sync::Mutex` unless holding across `.await`. Follow the lock order in `CLAUDE.md`.
- `Result<T, String>` for Tauri command errors. Custom error enums only when callers branch on variant.
- Modules have single responsibility. `mod.rs` >500 lines = split.
- Tests inline in `#[cfg(test)] mod tests` at bottom of the source file.
- `#[tauri::command]` and REST/Socket handlers stay thin — delegate to `ServiceContext` or `dispatch::dispatch`.
- No `unwrap()`/`expect()` in library code unless a documented invariant.
- Structural patterns over hardcoded strings for detection.

## TypeScript + React specifics

- Custom hook for any logic that touches refs, effects, or subscriptions. Components are mostly JSX + handlers.
- `useRef` for values that don't trigger re-render. Prefer refs over state for imperative APIs (xterm, mutex-like state).
- `useCallback`/`useMemo` only when there's a measurable perf problem.
- Effect dependencies: complete, no `eslint-disable`. If exhaustive-deps complains, the effect is buggy.
- StrictMode-safe: guard side-effectful `useEffect([])` with `useRef<boolean>` if you can't make it idempotent.
- No `any`. Use `unknown` + narrowing or a typed cast.
- Components >400 lines = extract hooks. Hooks >200 lines = extract pure functions to `lib/`.

## Architecture guardrails

- Adapters (REST, Socket, Tauri IPC, CLI) never contain business logic. They deserialize → dispatch → serialize.
- Business logic lives in `service/` or the target `dispatch::Command` layer.
- Daemon subsystems live in `service/`; there is no `services/`.
- Core (`multiplexer`, `pty`, `mesh`, `config`) has no outward dependencies.
- Every operation reachable from 3 transports (REST + Socket + Tauri) should live in `dispatch::Command` — not duplicated 3 times.
- Plugin-ready = isolated module with explicit interface. See `essences/contributing/architecture.md`.
- Design artifacts (screenshots, video) go in PR descriptions, never the source tree; release binaries come from CI only.

## Dependency advisories

Weekly audits (`cargo audit` + `pnpm audit`) flag known vulnerabilities. Assess reachability: if a vulnerability is in a transitive dependency your code never touches, document it with a bump-rationale comment in the PR; if reachable, upgrade or pin the affected version. Do not ignore advisories.

