# Begin here: the documentation contract

Read this before creating or editing any document in this repository.

## Two areas, split by audience

- **`essences/`** is the internal foundation for building CodeTerm and never ships: the vision, the capability map, this contract, the build, release and testing references, the plugin ecosystem map, the generated CLI reference and the contributor guides.
- **`prod-docs/`** is what production agents read. Each file is `prod-docs/<NAME>.md`, embedded in the binary through the docs registry and served by `codeterm docs <NAME>`; nothing else lives there, and a test holds that membership in both directions.
- **Skills** in `.agents/skills/` own the step-by-step procedures an agent executes. They are not a third docs area: an essence that needs a procedure points to its skill instead of repeating it.

## Kinds of documents

- **Stable docs** describe what a capability is and the contracts around it: the capability map, the core essence (core parts, clients and the routing spine) and one detailed doc per area. They name UI surfaces, command groups and API areas, never code identifiers, file paths or line numbers.
- **The project map** is generated from the capability map and the core essence: vectors, capabilities, core parts and clients, linked by runs on, feeds, part of and client of, within about one hundred nodes. When it grows past that, simplify the essences; never reshape the architecture to fit the map.
- **Reference docs** are the one internal source for building, releasing and testing. They may name commands and files; the executable steps live in the matching skill.
- **The vision** (`essences/VISION.md`) says why the capabilities exist.

## When to edit a stable doc

Only when a capability changes how it works: its behaviour, its entry points or its invariants. That change updates its capability map entry and the one detailed doc for its area together.

Never edit a stable doc to restate code, narrate a refactor, record a fix, add the example of the day or tidy wording.

## Where everything else belongs

| Information | Home |
|---|---|
| Implementation detail | the code |
| How to build, release and test | the build, release and testing references in `essences/`, and the matching skill |
| Decisions and incident history | plan strides and commit messages |
| Transient work | the plan |

## When a doc and the product disagree

Verify the product first, then fix whichever is wrong. A doc edit never papers over a product bug.

## Every change

- Bump the revision tag at the bottom of each doc you change; write in English.
- A document production agents read goes in `prod-docs/`, is registered in the docs registry and stays within its reading limit; every other document goes in `essences/`. There is no third docs directory.
- Prefer deleting stale or duplicated text over rewording it.

The map itself: `essences/CAPABILITIES.md`.

<!-- revision: 3 -->
