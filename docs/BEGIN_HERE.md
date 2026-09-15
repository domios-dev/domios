# Begin here: the documentation contract

Read this before creating or editing any document in this repository.

## Kinds of documents

- **Stable docs** describe what a capability is and the contracts around it: the capability map and one detailed doc per area. They name UI surfaces, command groups and API areas, never code identifiers, file paths or line numbers.
- **Procedure docs** are the one operational source for building, releasing and testing. They may name commands and files.
- **The vision** at the repository root says why the capabilities exist.

## When to edit a stable doc

Only when a capability changes how it works: its behaviour, its entry points or its invariants. That change updates its capability map entry and the one detailed doc for its area together.

Never edit a stable doc to restate code, narrate a refactor, record a fix, add the example of the day or tidy wording.

## Where everything else belongs

| Information | Home |
|---|---|
| Implementation detail | the code |
| How to build, release and test | the build, release and testing docs |
| Decisions and incident history | plan strides and commit messages |
| Transient work | the plan |

## When a doc and the product disagree

Verify the product first, then fix whichever is wrong. A doc edit never papers over a product bug.

## Every change

- Bump the revision tag at the bottom of each doc you change; write in English.
- A new top-level doc is either embedded in the docs registry or excluded from it with a reason; embedded docs stay within their reading limit.
- Prefer deleting stale or duplicated text over rewording it.

The map itself: `codeterm docs CAPABILITIES`.

<!-- revision: 1 -->
