# API liveness mesh load

This bench proves that a sparse transcript read, health and tab reads, and mesh
relay delivery stay responsive under concurrent load. It also proves that a
duplicate relay acknowledgement does not deliver the report twice.

## Run

Run against an exact-head dev daemon on port 7686:

```bash
bash evals/api-liveness-mesh-load/run.sh
```

## Pass

The final two lines begin `PASS:`. They report bounded latency, exactly one
`report,heartbeat` delivery, and that the sparse transcript work remained
responsive.

<!-- revision: 1 -->
