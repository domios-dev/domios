---
name: crate-split
description: Move modules out of the src-tauri app crate or codeterm-types into their own crates without breaking the build. Use for any roadmap step in docs/architecture/split-roadmap.md.
---

# Crate split checklist

Every failure below happened in the first split waves. Check each one before you report.

## Before moving

- Read `docs/architecture/split-roadmap.md` and the consumer set of what you move; a module that uses something staying behind creates a cycle. Measure it, do not trust the roadmap row.
- Move files with `git mv`. Never retype moved code: retyping dropped the `r` from a raw string and broke `\\.\pipe\` escapes.

## After moving

1. **Relative include paths.** Every `include_str!`, `include_bytes!` and fixture path is relative to the file. A file that moved to a different depth needs its `../` count recomputed. Verify each one: `ls "$(dirname <file>)/<path>"`.
2. **External dependencies.** Every external crate the moved code uses (`url`, `hostname`, `serde_json`, ...) goes into the new crate's `Cargo.toml` with the version the app uses. The app declaring it does not help the new crate.
3. **Visibility.** `pub(crate)` and `pub(super)` items reached across the new boundary become `pub`; a `pub(super) use` re-export of an item needs the item visible at least that wide.
4. **Imports.** Rewrite every path, including grouped `use a::{b, c}` and test modules. Leave a `pub use` facade in the old location only when it keeps callers unchanged.
5. **Lockfiles.** Do not hand-edit. Run `cargo metadata --format-version 1 --offline` in `src-tauri` (and `cli` when the CLI depends on the crate) to regenerate.
6. **Shared files.** In `src-tauri/Cargo.toml` members/deps, `codeterm-types/src/lib.rs` and `build.sh` scopes, only add or remove your own lines.

## Proof

Commit, then `./build.sh check-queue --wait-build` until it reports `errors=0`. Then report the SHA and the test selectors of every moved module with a non-empty filter. `module-graph.md` belongs to the map owner; mark your step done in `split-roadmap.md` only.

<!-- revision: 1 -->
