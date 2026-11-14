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

## The spec

1. Read the existing components and tokens first.
2. Write `spec.md`: views, states, copy and decisions. Every state has copy: empty, loading, error, disabled with its reason.
3. Build a throwaway playground on the real design tokens, plus a standalone HTML version. HTML on the real tokens is the source of truth, because implementers can lift its class names directly; no separate design tool.
4. Screenshot every view in dark and light at 1440, 480 and 390 px.

Present one opinionated draft plus the explicit open decisions. Side-by-side variant shootouts are not needed; when the owner must choose, ask one labelled A/B question.

## The review report

Review the real build, live, through the built-in browser: dark and light; 1440, 480 and 390 px; keyboard and accessibility snapshots; disposable data only.

Each finding is one numbered item:

- **Expected** and **actual**;
- a **concrete fix**: the token, state or copy to change (an item without a fix bounces);
- **blocking** or **non-blocking**.

Settle disputes with measurements (a pixel reading, a computed colour, a size), not with "looks fine". Recheck each batch of fixes until a final pass says there are no release-blocking deviations.

## Quality bar

- Every state has copy; no duplicated copy between a line and its footer.
- Nothing jumps when a filter or selection changes.
- Focus is visible and lands on the right element.
- Accessible names are clean and say what the control is.
- Phone layouts do not break titles or footers.
- Contrast holds in both themes.

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
