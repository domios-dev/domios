# Provider setup lifecycle oracle

This canonical-only drill verifies the three provider-setup promises against an
exact running build SHA. It uses fixture-supplied opaque task values and does not
write real provider hook files.

```bash
bash evals/provider-setup-lifecycle/run.sh --check
```

The drill prints independent PASS/FAIL rows for Codex hook repair/refusal,
Configure task delivery/failure, and installer command launch/failure. CodeTerm
CLI access is dev-locked through `dc`; the hook and failure branches use focused
fixtures rather than machine-global configuration.
