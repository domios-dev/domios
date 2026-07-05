---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3beeab4481e74716bd98e899b284e123
handle: phase-c-fix-regenerate-cli-manifest-docs
title: 'Phase C fix: regenerate CLI manifest/docs'
status: done
---

ROG Windows Phase C ran PowerShell foreground CLI cargo tests at exact integration SHA ce525b518c1edbd535106c630357de43e72342fa. CLI tests failed two freshness gates: cmd::manifest::tests::cli_manifest_is_fresh and cmd::manifest::tests::docs_cli_md_is_fresh. Error excerpt: docs/CLI.md auto-block stale - run codeterm cli-manifest --write.

Fix by regenerating the CLI manifest/docs from this SHA using the repo-supported command, committing only the generated doc/manifest changes required by the freshness tests. Do not touch unrelated CLI behavior.
