# Memory (mem)

`codeterm docs MEMORY-USAGE` is the command reference: CLI catalog, what to save as which kind, search fallbacks, wake-up scoring, pins and Sunday calibration. `codeterm docs MEMORY` covers storage, Supporter re-rank, the graph and reversible hygiene; `codeterm docs MEMORY-SERVICES` covers skill folders, the Librarian, workers and dreaming. Past sessions and keep-awake are in `codeterm docs SKILLS`. `codeterm mem --help` has the flags.

Use `codeterm mem taxonomy` to list areas with counts. Fold identical topics
with `codeterm mem area merge <from> --into <to> --dry-run`, then omit
`--dry-run` to apply. Scope is preserved. System areas, equal names and empty
sources are refused; a new destination is allowed. `mem history <id>` shows
the prior area; `mem restore <id> --version <history-row>` undoes that item.

<!-- revision: 8 -->
