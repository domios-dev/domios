---
tags:
- friction
---

Running 'codeterm plan list/get' against prod 7685 (old build lacking plan routes) yields RPC -32601 'Method not found' with no hint. Should detect missing route family and advise (e.g. 'this daemon has no plan routes; target the dev instance').