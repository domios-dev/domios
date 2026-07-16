# Manager bootstrap bench

Run only against an exact-head dev daemon:

```bash
source ../_lib/lib.sh
python runner.py --suite manager-bootstrap
```

The runner uses `dc` only. It creates a fresh orchestration pair and staffed
manager fixtures, sends the two identity probes, checks role/project plus the
honest unbound or Team context, verifies the worker control probe is unchanged,
then removes every created pane and Team. System-prompt and fallback fixtures
are recorded separately when the configured providers expose both capabilities.

Do not run while dev 7686 is stopped or its health build differs from `HEAD`.
