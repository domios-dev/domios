# Begin here: documentation contract

Read this before creating or editing a document in this repository.

## Documentation areas

- **Essences** contain stable product documents, plus the generated map.jsonl and append-only decisions.jsonl. The stable foundation is the [capability map](capabilities.md), [core essence](core.md), [vision](vision.md), and detailed stable contracts.
- **prod-docs** contains product help available through the CLI docs command group.
- **Skills** contain the step-by-step procedures agents use to complete operational tasks.

## Stable essence contract

Stable docs describe lasting product behavior and invariants through UI surfaces, CLI command groups, and API areas. They do not narrate implementation mechanics. Implementation code may refer to the relevant essences; essence documents link only to other essence files.

The capability map describes product vectors, capabilities, entry points, invariants, status, and relationships. The core essence describes core parts, clients, and message routing. The vision says why the product capabilities exist. The map is generated from the capability map and core essence; it is a navigation aid, not an architecture blueprint, and the architecture is never reshaped to fit it. The decision journal records why the map changed and is append-only.

## When to edit a stable doc

Edit a stable doc when product behavior, an entry point, or an invariant changes. Update the relevant capability entry and the detailed contract for its area together.

Do not use a stable doc to restate code, narrate a refactor, record a fix, add a temporary example, or tidy wording without a product change.

## Where other information belongs

| Information | Home |
|---|---|
| Implementation detail | The code |
| Step-by-step procedure | A project skill |
| Decisions and incident history | Plan strides and commit messages |
| Transient work | The plan |

## When a doc and the product disagree

Verify the product first, then fix whichever is wrong. A doc edit never papers over a product bug.

## Every change

- Write in English.
- Keep stable statements precise and avoid duplicating them across documents.
- Product help uses revision markers; stable essence docs do not. Prefer removing stale or duplicated statements over restating them.
- Link from an essence only to another essence file.
