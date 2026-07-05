---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_12c7cb2d66c14c2e862666b7ae527a88
handle: mobile-collapsible-compact-plan-header-phone-viewports-ms_12c7cb2d66c14c2e862666b7ae527a88
title: decision
---

SCOPE ADDITION (round-31b, user): the plan-view companion bubble currently renders the agent AVATAR (initials) — replace with a proper chat-FAB look: a message/chat-bubble ICON as the button face (lucide MessageCircle family, consistent with our icon set), with the agent's presence kept as a small status ring/dot on the bubble edge and the agent's name in the tooltip only. Same size/position/behavior otherwise (popover unchanged). Mobile relevance: on phone the bubble must not overlap the compact header's expand affordance or the composer — verify placement at 390x844 in this lane's scenarios (add assertion: bubble does not intersect the header bar or input). Criterion addition: companion bubble face is the chat icon, presence as ring; avatar/initials no longer the face.
