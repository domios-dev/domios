# Gate B attempt for the Chair to append

Append this row to `poc/eval/retrieval_e2b/ATTEMPTS.md` on poc main. This lane does not edit that repository. Earlier failed attempts remain unchanged.

| attempt | task / commit | config | held-out set | numbers (A3 recall@1/3/5 on held-out) | G1–G5 | Owner gate B (recall@5 ≥ 0.80 held-out) |
|---|---|---|---|---|---|---|
| B-domios-2 | BLU-14, Domios `evals/mem-embedder/b4-confirmation.json` @ ca4cbb8a2a19a405f20fd2d1a095ef79491dcf0f (embedder PREREG c203284a4073054ee0a936c6f973f30611bf7182; owner KEEP L12 pick c013885e6692f8c3bf4fea2a1f740ab46b5852a6; B1 PREREG 88f2e63b042dbe91b3b7cbba99bd849b636ab766; frozen e5f666bb37da53c9e5639d9d72901ad68bc1cdaf) | C10: global L12 at 512, pooled max-sim intents, weighted score fusion, enrichment, no rerank; fixed full-corpus BLU-12 harness | b4 (40 queries), SHA256 3d9da7cd5d10ff345b5628a18f61151c03138893aab3dd4ab51bb6f5d2a4ec6b verified against B2 MANIFEST after pick | 0.8500 / 0.7000 / 0.8166666667 (degraded @5 0.7833; optional rerank not measured) | Not evaluated under PoC G1–G5 | PASS (0.8166666667 ≥ 0.80), exactly one invocation, 2026-10-22T15:24:17.102883+00:00 |

The local scratch source was `270bdbb18b3b3b23e6934b4694d0db997682d534`; the raw command exited 0 in gateway job `20261022T152352-58369-17577`. C10 SHA256 was `4dbb17ad4f8ccf306755a3ce707b0ac5adefd7d49428d741e9dff29896ffb432`. The optional paid reranker was disabled; the gate itself never uses reranking.
