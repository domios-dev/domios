---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_a4f6f84a20f645ae8b8bbdd234354b4e
handle: workspace-plan-entry-softlock-ms_a4f6f84a20f645ae8b8bbdd234354b4e
title: decision
---

soft-lock root cause = showSwitch gate (TabContent L63) hides whole PaneModeSwitch when no chat-capable agent. Fix: chatPty prop on PaneModeSwitch -> standalone Plan toggle when no chat surface; PaneTopBar always mounts switch. Board overlay already agent-independent (planActive && api).
