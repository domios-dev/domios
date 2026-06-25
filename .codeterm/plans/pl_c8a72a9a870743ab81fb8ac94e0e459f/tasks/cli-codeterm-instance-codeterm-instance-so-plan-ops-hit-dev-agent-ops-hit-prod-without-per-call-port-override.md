---
tags:
- friction
---

Global codeterm CLI defaults to PROD 7685 with no first-class instance selector. Orchestrator must export CODETERM_API_PORT=7686 + discover dev token per call for plan ops, but keep 7685 for agent spawn/report. Easy to fat-finger. Want: 'codeterm --instance dev <cmd>' or honor CODETERM_INSTANCE to resolve port+token+socket. High dogfood friction.