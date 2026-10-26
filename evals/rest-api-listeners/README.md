# evals/rest-api-listeners — REST API listener lifecycle

Sixteen `startup::rest_api` tests in `codeterm-service-core` that bind real sockets: bind and publish
(including the mesh tailnet listener), manual and overlapping restarts, port release and foreign owners,
unexpected exit, health-proof deadlines and outage diagnostics. They are a curated eval, not part of the CI
test suite: run them when the REST API listener, restart or port-release code changes.

## Run

```bash
evals/rest-api-listeners/run.sh
```

They run as one nextest pass on their own, because children forked by other tests inherit the sockets and
keep the ports held (#620).
