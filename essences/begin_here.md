# Begin here: the documentation contract

Read this before creating or editing any document in this repository.

## What essences are

`essences/` is the stable foundation: what CodeTerm is and the contracts that must
keep working. The vision is itself an essence — a stable doc. Essences change
rarely and only when a capability changes how it works.

## What lives in essences

Only three things:

- **Stable docs** — what a capability is, its entry points and its invariants. They
  name UI surfaces, command groups and API areas, never code identifiers, file paths
  or line numbers.
- **The project map** (`essences/map.jsonl`) — generated from the capability map and
  the core essence. When it grows past about one hundred nodes, simplify the
  essences; never reshape the architecture to fit the map.
- **Decisions** (`essences/decisions.jsonl`) — the durable decision log.

Nothing else. A procedure, a reference, an incident write-up or a generated artifact
is not an essence.

## Rewrite before you delete

When content in `essences/` is broken — a stale area doc, a how-to wearing an
essence's name, a reference with no stable core — rewrite what it holds into a proper
stable essence. Delete only what genuinely cannot be expressed as one. Prefer
rewriting over deleting; prefer deleting over rewording.

## References

Inside `essences/` a link may point only to another essences file. The code
references the essences docs that explain its contract — code points at essences,
never the reverse.

## When to edit a stable doc

Only when a capability changes how it works: its behaviour, its entry points or its
invariants. That change updates its capability map entry and the one detailed doc for
its area together.

Never edit a stable doc to restate code, narrate a refactor, record a fix, add the
example of the day or tidy wording.

## Where everything else belongs

| Information | Home |
|---|---|
| Implementation detail | the code |
| How to build, release and test | `.agents/skills/` and the references those skills own |
| A document production agents read | `prod-docs/`, registered and within its reading limit |
| Decisions and incident history | `essences/decisions.jsonl`, plan strides and commit messages |
| Transient work | the plan |

## When a doc and the product disagree

Verify the product first, then fix whichever is wrong. A doc edit never papers over
a product bug.

## Every change

- Bump the revision tag at the bottom of each `prod-docs/` doc you change; essence
  docs carry no revision tag. Write in English.
- A document production agents read goes in `prod-docs/`, is registered in the docs
  registry and stays within its reading limit; every other document goes in
  `essences/`. There is no third docs directory.

The map itself: `essences/capabilities.md`.
