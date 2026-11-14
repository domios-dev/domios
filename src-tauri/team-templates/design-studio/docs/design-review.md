# Design spec and review

How design work ran on real product surfaces: one opinionated draft on the real tokens, implementation by separate Workers, and live review of the actual build until nothing blocking was left.

## The brief

The Lead writes it from the owner's words. A good brief holds:

- the surface and every flow to cover;
- the real data model behind it (fields, states, what can be empty or fail);
- the owner's tone rules and anything banned (the suggested default: quiet and calm, the existing components, no decoration nobody asked for);
- what already exists to reuse (components, tokens, shared rows and buttons);
- the decisions still open, and who decides each one;
- where the output goes: drafts in the pad first; product source only after the owner approves the draft and allows source changes;
- the deadline, if the owner named one.

Undecided behaviour discovered during review cost extra rounds every time. Decide trust, consent and where each choice lives in the brief.

## No design system yet

Most projects have tokens and components; reuse them. When a project has none (a new landing page, a first product surface), settle a direction before any screen:

1. Take what the README, site and existing assets already answer. Four things are required: the exact product name, a one-sentence pitch, the audience, and the one action a visitor should take.
2. Ask only what is still missing, one question at a time and about ten at most, offering concrete choices: three words for how it should come across, light or dark first, and a direction for colour, type and shape (sharp or rounded). State the defaults you chose for the rest.
3. Write the result as a small tokens draft in the pad: colour roles for both themes, a type scale, spacing, radii. It is checked against the quality bar's contrast rules before the owner sees it.
4. The owner approves the tokens draft; every later screen builds on it.

When the owner's notes are pixel-level ("40 px more above the title"), apply them and also write down the intent behind them ("more room above the headline"), so later screens follow the intent.

Adapted from the brand interview in wshobson/agents `brand-landingpage` (MIT), without its Stitch dependency; the wording is ours.

## The spec

1. Read the existing components and tokens first.
2. Write `spec.md`: views, states, copy and decisions. Every state has copy: empty, loading, error, disabled with its reason.
3. Build a throwaway playground on the real design tokens, plus a standalone HTML version. HTML on the real tokens is the source of truth, because implementers can lift its class names directly; no separate design tool.
4. Screenshot every view in dark and light at 1440, 480 and 390 px.

Present one opinionated draft plus the explicit open decisions. Side-by-side variant shootouts are not needed; when the owner must choose, ask one labelled A/B question.

## The review report

Review the real build, live, through the built-in browser: dark and light; 1440, 480 and 390 px; keyboard and accessibility snapshots; disposable data only.

Start from "not passed". Judge what the rendered build shows, not the code, the spec or the implementer's description of the change.

Each finding is one numbered item:

- **Expected** and **actual**;
- a **concrete fix**: the token, state or copy to change (an item without a fix bounces);
- **blocking** or **non-blocking**. A quality-bar failure that stops someone from using the surface (no keyboard access, no accessible name, unreadable contrast) is blocking.

Settle disputes with measurements (a pixel reading, a computed colour, a contrast ratio, a size), not with "looks fine". Recheck each batch of fixes until a final pass says there are no release-blocking deviations.

When several reviewers or rechecks report the same problem on the same element, keep one item with the more detailed description; the higher severity wins. Different problems on one element stay separate items.

The stance and merge rule follow wshobson/agents `ui-visual-validator` and `multi-reviewer-patterns` (MIT); the wording is ours.

## Rounds

Keep a round log in the pad: each owner round numbered, the owner's notes verbatim, and the answer to each (done, changed, declined with the reason). Screenshots are stored as files and referenced by path. A resumed or replacement Worker reads the log before anything else; summaries lose the small notes.

## Quality bar

- Every state has copy; no duplicated copy between a line and its footer.
- Nothing jumps when a filter or selection changes.
- Focus is visible, lands on the right element and is never hidden behind a sticky header, footer or overlay.
- Accessible names are clean and say what the control is.
- Phone layouts do not break titles or footers.
- Contrast holds in both themes: at least 4.5:1 for body text, 3:1 for large text, and 3:1 for control boundaries, icons that carry meaning and focus indicators.
- Pointer targets are at least 24 by 24 CSS px, or spaced so a 24 px circle around each touches no other.
- Colour is never the only signal: errors, states and links also differ by text, icon or shape.
- Content reflows at 320 CSS px without sideways scrolling, and text zoomed to 200 % still fits.
- Motion respects the reduced-motion setting.

These follow WCAG 2.2 level AA (the reduced-motion line is our own rule). Automated scanners find only part of the problems; the keyboard walk and the screen-reader names are checked by hand.

The concrete WCAG floor was suggested by wshobson/agents `wcag-audit-patterns` (MIT); the criteria are the W3C's.

## Red lines

- Never edit product source from a design task; drafts live in the pad. Implementers build from the spec and the review items.
- Never invent or alter brand or third-party assets; partner snippets and badges stay verbatim.
- Never test in the owner's real workspaces or type into live agent tabs; use fixtures or disposable data.
- Never touch a production instance; review on a development or preview build.
- Secrets only through stdin or the secret store.
- Never mark something passed without seeing it on the actual build.
- No banners, badges or decoration the owner did not ask for.
- Always clean up: disposable data, templates, profiles, the browser, and the theme back to the owner's default.

## Day one

- Confirm the build you are reviewing is the one you expect, by its full commit SHA; a short SHA can be refused. A stale bundle makes an already landed fix look failed, so rebuild before every recheck.
- Phone widths can need different navigation (for example opening a collapsed sidebar first).
- Review items without a concrete fix bounce. Always write the fix.
