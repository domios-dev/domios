# Blueprints for recurring actions

Before you repeat a check or wait on an external, agent or cron status, build a
meaningfully named Blueprint graph: what is watched → what happens. Start with
`codeterm blueprint recipe [<name>]` or the patterns in `codeterm docs BLUEPRINT-RECIPES`,
validate create or update with `--dry-run`, and read `codeterm docs BLUEPRINTS` for
sources, buckets, handlers, escalation, versioned updates and limits.
`codeterm blueprint create --help` has the flags.

Creation is admission, not proof of monitoring. Run the read-only probe once and
check its exit status and exact output, including LF/CRLF. After creation, use
`blueprint show` to verify an actual source execution and the expected recognized
bucket; when a route fires, verify its retained transition and handler result.
An unclassified watch needs repair before reporting success. Distinguish probe
watches from pipelines: an empty Runs view alone proves neither failure nor a
UI bug. Continue through events or routed results, never a sleep or polling loop.
For GitHub, start with `github-ci-watch` or `github-publication-watch` recipes;
scope CI to an exact run and publication to an exact tag and required assets.

An input stage watches data instead of a probe: `--input <text|audio|video|image>`
with `--input-file`, `--input-url` or `--input-text`, and optional `--convert-to <text|audio>`.
See `codeterm docs BLUEPRINTS` for the full input stage reference.

<!-- revision: 9 -->
