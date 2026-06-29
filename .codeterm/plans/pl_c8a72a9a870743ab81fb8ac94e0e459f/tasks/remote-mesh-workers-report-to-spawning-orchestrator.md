---
tags:
- orchestration
- mesh
- release
---

Remote-mesh-worker release report remains in_review pending release run completion. Additional safety finding from worker: build.sh kill prod was not introduced by release branch; it is existing env-inheritance footgun. build.sh uses CT_PORT=7685; kill_dev uses that port. In prod pane CODETERM_API_PORT=7685, so ./build.sh kill can target prod despite safety comment. Separate safety task created; worker will not run build.sh kill again.