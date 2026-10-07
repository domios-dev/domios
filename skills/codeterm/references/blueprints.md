# Blueprints for recurring actions

Before you repeat a check or wait on an external, agent or cron status, build a
meaningfully named Blueprint graph: what is watched → what happens. Start with
`codeterm blueprint recipe [<name>]` or the patterns in `codeterm docs BLUEPRINT-RECIPES`,
validate create or update with `--dry-run`, and read `codeterm docs BLUEPRINTS` for
sources, buckets, handlers, escalation, versioned updates and limits.
`codeterm blueprint create --help` has the flags.

An input stage watches data instead of a probe: `--input <text|audio|video|image>`
with `--input-file`, `--input-url` or `--input-text`, and optional `--convert-to <text|audio>`.
See `codeterm docs BLUEPRINTS` for the full input stage reference.

<!-- revision: 8 -->
