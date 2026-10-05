# Sentinels for recurring actions

Before you repeat a check or wait on an external, agent or cron status, build a
meaningfully named Sentinel graph: what is watched → what happens. Start with
`codeterm sentinel recipe [<name>]`, validate create or update with `--dry-run`,
and read `codeterm docs SENTINELS` for sources, buckets, handlers, escalation,
versioned updates and limits. `codeterm sentinel create --help` has the flags.

<!-- revision: 5 -->
