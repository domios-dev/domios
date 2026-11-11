# Bet card, scoring and output

## What a bet is

A bet is one outcome sized by the `bet_size` input, visible outside the people who build it. Bets group into the regions of the `regions` input; a region's hub bet is the one the others in it build on. Each bet has a stable code: its region code plus a number.

Gates (an approval, a release, an external dependency) and design-owner items (a decision only the design owner can make) are nodes of their own, with codes, so bets can be blocked by them.

## The card

One YAML card per bet, with these fields:

- `code`, `title`, `region`
- `outcome`: what is true for users when the bet lands, in one or two sentences
- `size`: the estimate, in the unit of the `bet_size` input
- `wave`: 1 when no other bet blocks it, 2 when it builds on a wave 1 bet
- `blockedBy`: codes of the bets, gates and design-owner items it needs
- `done`: done lines, each one observable and checkable on its own
- `nonGoals`: what the bet deliberately leaves out
- `risks`: what could sink it
- `evidence`: `file:line` references in the repositories and links to design-record elements that ground the card

Each bet also gets a spec one-pager in the style of the `spec_style` input, plus notes that carry the `file:line` evidence.

## Scoring

Every bet is scored after simulation:

- **Score 1-10**: the expected value of the bet given what the simulation found, where 10 is a sure, high-value outcome and 1 is not worth starting.
- **Top three assumptions**: the three beliefs the score depends on most, weakest first.
- **Kill criterion**: one observable signal, checkable early, that says stop this bet.
- **Done lines**: each judged `met`, `partly` or `not` from what the pod actually delivered, with the reason. Never round `partly` up.
- **Real size, blockers and spec changes**: what the simulation found, next to the estimate.

## Output of a run

1. The bets ranked by score, each with its score, top three assumptions, kill criterion and done-line judgments.
2. A navigable map of bets, gates, regions and blockedBy chains (see the bet-map skill).
3. An export of the whole set in Markdown, and the plan doc with a Mermaid blockedBy graph.
4. The owner-decisions log.
