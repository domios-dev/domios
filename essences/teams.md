# Team templates: portable teams on one start rail

A Team template is one package in the Agentic Team Protocol (ATP): a portable description of a Team's roles, their instructions and model preferences, what the Team asks of its host, and what it brings with it. Every Team starts from a template, the builtin default unless the owner names another, through the one existing Team creation path. Templates describe a way of working; they carry no live state, plans, secrets or machine details. [Orchestration](orchestration.md) owns the roles and authority a started Team runs with.

## One package, three forms

The same package exists as one inline YAML file, as a ZIP archive or as a folder; the two archive-shaped forms hold a root `team.yaml` plus only the files it references. One shared schema, parser and validator reads every form for the daemon, the CLI and any external validator, so the same input yields the same document and the same findings everywhere. Findings carry stable codes and locations. A malformed or unsafe package is refused before anything starts or is saved; a portable but incomplete one (a role naming a skill the package lacks) is valid with a warning. Host gaps, such as a missing harness, tool or secret binding, are runtime needs shown at start, distinct from format errors.

One canonical writer produces every export, so export, import and export again yields the same bytes in every form.

## Roles

Manager slots are fixed and carry host-known duties: one orchestrator, and optionally a planner, cartographer and curator. Worker types have free names; a worker named like a host builtin replaces it, and builtins the package does not name stay available. Each role carries its own instructions; there is no package-wide instruction or brief, and the Team's way of working lives in the orchestrator's instructions. Host runtime text is appended by the host and can never be removed or replaced by a package, so a package grants no authority.

A role's model is an ordered preference list, a reference to another role's resolved model, or left to the owner (managers) and the normal defaults (workers). Only the managers that start with the Team start; every other role is registered and dispatched on demand.

## Consent and provenance

Packages received from others stay unverified with no default grants. Starting an unverified package, or one that needs new grants, requires the owner's recorded decision, enforced by the host rather than by the interface. Package updates never change an active Team's registrations. Lineage records which package a baked template was derived from.

## Scope

Version one has no plan, spec or task templates and never exports plans.
