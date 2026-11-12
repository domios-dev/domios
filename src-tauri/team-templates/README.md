# Builtin team templates

Each builtin template is one Agentic Team Protocol (ATP) package in folder form:

```
team-templates/
  <package>/
    team.yaml          the package document (exactly one, at the folder root)
    managers/*.md      instructions_file targets for manager slots
    workers/*.md       instructions_file targets for workers
    ...                any other file team.yaml references (icon, files, skills, automation)
```

`codeterm-team-package` embeds this whole tree and indexes every folder that holds a root `team.yaml`, so adding a builtin template means adding its folder: there is no registry to edit. A folder may hold only files its `team.yaml` references; every builtin must pass `domios team check <folder>`.

The format is documented by `domios docs TEAM-FORMAT`.

This tree is licensed under Apache-2.0 (see `LICENSE` here), like the `codeterm-team-package` crate that embeds it; the rest of Domios is AGPL-3.0-only.
