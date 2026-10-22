# Decision: keep L12 (owner)

The owner explicitly chose **KEEP L12** after seeing the interim candidate evidence: there is no gain worth a global switch, and simpler is better. This owner decision supersedes the original pre-registered selection procedure and the intervening owner amendment. The decision is committed before any b4 query is read, copied or hashed. Global default remains sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2, mean pooling, no prefixes, 512 tokens and 384 dimensions.

## Candidate evidence

B3 and Russian values below are post-hoc family recall from B1's unchanged frozen C10, built on the local real scratch merge. B3 has 38 queries; Russian has 20. Compute values from raw per-query fractions rather than rounded harness summaries.

| Model | b3 R@1 | b3 R@3 | b3 R@5 | Russian R@5 | Delta Russian vs L12 | Disk MB | Oracle min cosine |
|---|---:|---:|---:|---:|---:|---:|---:|
| L12 | 0.842105 | 0.771930 | 0.921053 | 0.733333 | 0 | 479.725 | 0.999999821 |
| multilingual-e5-small | 0.894737 | 0.807018 | 0.868421 | 0.783333 | +0.050000 | 487.726 | 0.999999881 |
| bge-small-en-v1.5 | 0.868421 | 0.833333 | 0.947368 | 0.333333 | -0.400000 | 134.179 | 0.999999881 |
| e5-small-v2 | 0.973684 | 0.815789 | 0.929825 | 0.450000 | -0.283333 | 134.179 | 0.999999881 |

Each oracle checks 70 passage/query cases including Russian, Unicode and truncation. All exceed the predeclared cosine >=0.999 tolerance. Model revisions, required-artifact hashes, per-case errors and public reference vectors are retained. Throughput is informational and recorded separately in throughput.json. Initial oracle timings overlapped compilation for some candidates; the separate throughput-only run follows cancellation of the build and is the reported timing source.

BGE's Blueprint gain is 0.026316, with a 0.40 Russian recall loss. E5-v2 gains 0.008772 Blueprint recall and loses 0.283333 Russian recall. Multilingual-e5 gains 0.05 Russian recall but loses 0.052632 Blueprint recall. The owner chooses simplicity and the existing global model despite the smaller English-only alternatives and their Blueprint gains.

## Decision provenance and scope

1. PREREG c203284a4 was committed before all candidate measurements. Its original bounds included mem-bench Recall@5/MRR and Russian recall. Measured b3/Russian evidence already rules out the English candidates under its Russian bound, while multilingual-e5 trails L12 on b3. L12 is the only possible winner from these measured arms; the complete original rule was not evaluated because ordinary mem measurements were subsequently dropped.
2. After those interim numbers, the owner briefly removed Russian as a selection criterion and described a best-b3/ordinary-mem-Recall@5 rule, with L12 winning ties. That amended verdict remains unevaluated because ordinary mem-bench was not run.
3. Before any pick was committed, the owner superseded that amendment with the final **keep L12 (owner)** decision. The owner explicitly dropped mem-bench, prod-snapshot recall and fresh query writers. Their missing results are disclosed, never counted as passes. B4 is now a single real L12 gate-B confirmation for the owner, not validation of a completed automatic embedder selection.

No default changes and no existing-store model migration are required. The default-dependent legacy_identity pin is deferred to any future default change. The trait, switch, CLI and UI remain unchanged. An additional migration fixture was prepared and passed once during the wider task, then removed with the dropped scope; it is not claimed as evidence of a model switch.

## Snapshot cleanup

The read-only Chair snapshot was filtered into one worker-owned private directory: 606 embeddable items retained, five credential matches and two short items excluded, no Blueprint-library solutions. Only aggregate preparation counts are retained. A deterministic 150-item sample was prepared; **zero subject sessions ran**. The entire worker-created private corpus/sample directory was deleted and absence verified. No live production memory or daemon was touched. The Chair keeps custody of, and deletes, the source snapshot.

## Next action

After this decision commit, verify B2's sealed b4 SHA256 against its MANIFEST, then run fixed B1 `--confirm --set b4` exactly once, using frozen C10 and the global L12 provider. Record PASS or FAIL without another attempt; the threshold is recall@5 >=0.80. The final report supplies the ATTEMPTS.md row for the Chair to append on poc main. Nothing is pushed.
