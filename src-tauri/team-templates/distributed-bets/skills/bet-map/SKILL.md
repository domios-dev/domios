---
name: bet-map
description: Build a navigable map of bets, gates and regions from YAML data, with a validator, a Markdown export and chain tracing.
---

# Bet map

The map is a small local app over plain data files, so the bets stay reviewable as text and the map is always regenerated from them.

## Data

Keep one YAML file per concern in the project's bets folder:

- `bets`: every bet card (see docs/bet-card.md)
- `gates`: gates and design-owner items, with codes
- `inbox`: open questions that could still turn a bet, each naming the bet it could turn
- `regions`: region codes, names and hub bets
- `vocab`: the product terms the cards use, one definition each
- `order`: the display order of regions and bets

## Validator

Add a single check command to the map app (for example an npm `check` script) and run it after every data change. It fails when:

- a code is duplicated, or a `blockedBy`, region or inbox reference points to a missing code;
- the blockedBy graph has a cycle;
- a wave 1 bet is blocked by another bet, a wave 2 bet by another wave 2 bet, or a card misses a required field;
- a scored bet lacks its score, three assumptions, kill criterion or done-line judgments.

## Views

- Layouts: by region, as a dependency tree, and radial around a selected bet.
- Chain tracing: selecting a bet highlights everything it needs and everything that needs it.
- Side panel: the selected card in full, with its score, assumptions, kill criterion, done-line judgments and evidence links.
- Ranking: bets sorted by score, filterable by region and wave.

## Export

Generate a Markdown export of every bet in rank order from the same data, and a plan doc with a Mermaid `graph` of blockedBy edges. Never edit the export by hand; fix the data and regenerate.
