---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_5b90dda0599b4594931767098329fad4
handle: mobile-collapsible-compact-plan-header-phone-viewports-ms_5b90dda0599b4594931767098329fad4
title: decision
---

SCOPE ADDITION (round-34, user + screenshot /Users/rollacode/.codeterm/uploads/0a01f5e1-CodeTerm.heic): CHAT MESSAGES GO FULL-WIDTH ON MOBILE. On narrow viewports the message bubbles keep the desktop max-width proportion (~65-70%), wasting a third of an already-small screen and making messages read 'shortened'. Rule: below the md breakpoint, bubbles span the full content width (edge padding only, ~12px); alignment stays (user right / agent left) via padding asymmetry or a slim alignment accent, not via width reduction. Applies to user and agent bubbles, cards (reminder/intro), and code blocks inside messages (code blocks already suffer worst from the narrow column). Playwright addition at 390x844: bubble content width >= 90% of the chat container width for a long message. SECONDARY finding in the same shot: a codeterm:reminder marker rendered as RAW TEXT inside a message bubble instead of the structured Reminder card — markers embedded in message bodies must render structured wherever the top-level path already does; add a component test with a marker mid-body.
