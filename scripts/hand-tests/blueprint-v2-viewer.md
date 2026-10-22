# Isolated Blueprint v2 viewer check

Integration owner: BLU-38. Run on the final clean integration SHA after the accepted call chain and viewer lane are real-merged. This procedure does not launch agents or start DEV. Use BLU-38's isolated named DEV, exact SHA/readiness receipt and disposable workspace; never production or ports 7685/7686. Use the Domios Browser plugin for UI steps. Keep the redacted transcript and screenshots outside the source tree.

## Preconditions and command oracle

Use the accepted live intent harness's saved, pinned typed caller with at least one nested callee and retained runs. Record the caller ID, definition version and source SHA. Capture these through that isolated instance's configured CLI:

```bash
domios blueprint show CALLER_ID --json
domios blueprint log CALLER_ID --limit 200 --json
domios blueprint history CALLER_ID --json
```

The observed `call_graph`, `expanded_calls` and per-run `nested_run` are the oracle. Do not replace an absent stop or callee with a handwritten transcript. Missing pins, missing retained native definitions, or unavailable nested transport fail this row. The CLI and viewer must use the same bounded/redacted values.

## Viewer and selected-run proof

1. Open Jobs, then the saved caller. Verify accessible node controls and actual forward skip routes against the admitted document, including an edge that skips an intervening node. Open node details and inspect both typed ports and code body.
2. Select the call node. Record solution ID/hash, declared inputs, result and body. Expand the callee and verify its nodes appear in place. Expand one further nested call if present; collapse/reopen and confirm parent/child identities never collide. Compare all values to `blueprint show` output, including redaction/truncation markers.
3. Select a retained failed run with a real nested `contract_violation`. Record the run ID, exact stop node/reason and full call stack. Select a different run and confirm journal rows change with it; failed historical evidence must not appear on the latest running run. Expanding a callee in a retained run uses that run's retained graph/hash, not a newer definition. An unavailable retained definition must say so.
4. Start a disposable run through the accepted harness's actual product trigger/input path. Wait for authoritative running/journal evidence. Select a historical run and confirm Stop selected run is disabled. Select the current running run, stop through the button, then wait on the graph-changed event and the retained journal/CLI state. Confirm the run has actually stopped. A button click or stop request acknowledgement alone is not proof.

## Semantic diff proof

1. In the disposable workspace, use the product's explicit versioned update path to create an admitted second definition with changed route, port, capability, code body, import and call pin. Preserve the before/after native IR and CLI checker semantic diff. Do not grant additional execution authority merely to test a display.
2. Open Semantic diff, select the new definition version and compare node additions/removals, changed fields, imports and each callee pin/body diff with the backend projection. Select an older version and confirm the displayed comparison changes. If a predecessor was not retained, the viewer must identify that gap rather than imply no changes.

## Read-only repository proof

Use only the disposable repository. Create a valid native v2 `.domios/blueprints/health.yaml` whose name is `health`, one stem/name mismatch and an escaping file/directory link where the OS permits it. Also exercise duplicate stems with `.yaml` and `.yml`.

```bash
domios blueprint project list --json
domios blueprint project show .domios/blueprints/health.yaml --json
```

Open List repository Blueprints and View the valid entry. Verify source path/name/intent, typed graph and parameter assignments. Mismatch/escape/ambiguity must refuse. Confirm there are no run/apply/approve actions; compare the Blueprint store and run journal before and after list, view and checkout changes. No job, approval or effect may be created.

## Responsiveness and receipts

Use a real 24+ node document with nested calls, expand several callees and record the resulting node/edge counts. Capture browser Performance marks or a performance trace for initial model construction, expansion and React commit. During each operation type into a parameter input and scroll the graph. Record model total, longest work chunk, render/commit duration, input event latency and scroll event latency in milliseconds, plus any long tasks (>=50 ms). Report browser/OS/machine and load. Model work must yield between batches; no full serialization, parsing or graph clone may occur in render or input handlers. Node unit model timings alone are not a live-browser responsiveness PASS.

Archive the final SHA, exact commands/exits/test counts, source/oracle JSON, screenshots or a recorded UI transcript, performance numbers, actual persisted stop evidence and platform gaps. Integration runs the full frontend suite and api-effects once; Workers do not duplicate those gates.
