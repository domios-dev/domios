---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_19d01fbad60d45f9a4db9d5ef36b2cb2
handle: mobile-collapsible-compact-plan-header-phone-viewports-ms_19d01fbad60d45f9a4db9d5ef36b2cb2
title: decision
---

SCOPE ADDITION (round-34b, user + screenshot 7725abd5): TERMINAL TAB BLANK ON MOBILE WEB — the Terminal face renders an empty area on iOS Safari web-mode (toolbar + esc/arrows/enter helpers render, terminal canvas empty). VERIFY-FIRST: the user notes this may already be fixed — the PTY reader-race fix cured a blank-terminal class; reproduce on CURRENT dev from a phone (or Safari responsive mode) before coding. If still broken, likely suspects in order: (1) xterm renderer on iOS Safari (WebGL renderer unsupported/context-lost -> needs canvas/dom renderer fallback), (2) fit-addon sizing against a zero-height flex container on mobile layout, (3) the mobile keyboard-helpers bar changing the container height after xterm measured. Acceptance: on 390x844 web-mode, the Terminal tab of a live agent pane shows real PTY content within 2s of switching; add to this lane's Playwright scenarios (webkit project if available, else chromium mobile emulation) with a content assertion (non-empty rows in the xterm buffer).
