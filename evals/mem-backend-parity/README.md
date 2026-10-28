Default vectors are **synthetic (normalized hashed-token vectors)**. This bench measures storage and ranking parity, not neural retrieval quality.

Run after the 1.17.0 cut in canonical. No Domios daemon, owner session, API credential, live agent or model download is required. `run.sh` sources `evals/_lib/lib.sh` and uses its repository/output conventions; the library's best-effort DEV discovery does not have to succeed. Store calls go directly through `MemStore`, with no CLI or HTTP calls from the bench. Inherited `CODETERM_*` connection selectors are removed in the Rust process. The database address is fixed to the disposable loopback container below.

From the repository root in Bash, start the disposable pgvector server:

```bash
docker run --detach --name domios-mem-backend-parity --label domios.bench=mem-backend-parity --env POSTGRES_DB=domios_parity --env POSTGRES_USER=domios_parity --env POSTGRES_PASSWORD=domios_parity --publish 127.0.0.1:55439:5432 --tmpfs /var/lib/postgresql/data pgvector/pgvector:pg17
```

Run the bench once and retain its raw exit code and artifact directory:

```bash
bash evals/mem-backend-parity/run.sh
```

Then remove the container, including after a failed run:

```bash
docker rm --force domios-mem-backend-parity
```

The container name, image tag, label, loopback port binding and temporary data mount are checked before connecting. Readiness comes from the final PostgreSQL TCP startup event, bounded at 30 seconds, followed by a real store connection. No readiness polling or fixed warmup is used. Container and image IDs are recorded in `run.log`; the image tag can move, so preserve that receipt with results. The database credentials above are public disposable fixture values. No volume or owner database is reused. Each run owns a fresh schema, drops it on completion or error, and uses temporary SQLite databases that are removed when the harness exits. The external cleanup command removes the server even if the bench is interrupted.

The Cargo example is registered only in `codeterm-mem-postgres`; it uses existing dependencies and dev-dependencies. It does not add a shipped binary or change backend implementations. This is cheaper than running two application daemons and avoids changes to application backend configuration. The default build uses the existing SQLite exact-scan vector implementation and PostgreSQL pgvector cosine distance. It is a ranking benchmark, with no latency claim or performance comparison.

`corpus.json` contains 36 fixed English memory records across storage, terminal and network topics. `queries.json` contains 24 fixed queries: 12 exact titles, six natural questions, three misspellings and three filtered queries. Every query names its expected exact title, including natural and misspelled queries. Corpus and query SHA-256 hashes are printed. Neither the inputs nor the thresholds depend on observed backend results.

Synthetic vectors count lowercase alphanumeric tokens into 64 buckets. The bucket is the first two SHA-256 bytes interpreted as a little-endian integer modulo 64. Vectors are L2-normalized. Passage text uses the production `compose_embed_text` and preprocessing helpers. Local inference, when requested, embeds the same text. Every passage and query vector is computed once, then reused byte for byte for both stores. A frozen query provider keeps inference speed and the production query-embedding deadline from changing the inputs. A degraded query embedding is recorded as an error, not accepted as a successful lexical-only measurement.

An initial disposable SQLite store creates the fixture through the public save/vector APIs. Its exported rows are frozen, including fixed ranking timestamps and deterministic item UIDs, then imported identically into a fresh measured SQLite store and PostgreSQL. All exported rows must match before and after measurement. Metadata, IDs, text, vector stamps and vector bytes are therefore equal; recall uses `record_access=false` and unmodified `SearchConfig::default()`.

Each `QUERY` output line contains the top ten hits with IDs, titles and scores, their set Jaccard, and expected-title ranks for each backend. The main measurement calls recall with limit 10. A separate limit-50 diagnostic finds the full expected-title rank across this 36-item corpus; it does not supply the top-ten comparison. A null top-ten rank means the title was not in those ten results. A null full rank means it was not retrieved even at limit 50. Filters can produce fewer than ten hits; both cardinalities remain visible. Jaccard is intersection size divided by union size; two empty results score zero.

The provisional acceptance rule is median top-ten Jaccard **>= 0.7**, with the expected exact title at **rank 1 in both backends for every query**. The printed rank-one rates use all 24 queries as their denominator. `failing_queries` lists every query with a retrieval error, a rank-one miss on either backend, or an individual Jaccard below 0.7. Individual Jaccard misses are diagnostics; the aggregate Jaccard gate uses the median. Any retrieval error makes the overall median unavailable and fails the run, so failed queries cannot disappear from the aggregate. No threshold or ranking parameter is tuned by the harness.

`target-check/evals/mem-backend-parity.*/` holds `run.log`, `inputs.json` and `report.json`, including all per-query evidence and the aggregate summary. Exit 0 means the provisional aggregate rule passed, 1 means measured failure, 2 means setup/execution failure, and 77 means local embeddings were explicitly skipped. A skip never counts as parity evidence. No parity numbers are claimed until canonical executes the bench.

For a later real-vector run, select the local Candle model with `--local-model` (the production local default is used if omitted). Point `HF_HOME` at its existing Hugging Face cache, or leave it unset to use `CODETERM_EMBED_CACHE_DIR`/the normal Domios model cache. The bench forces `HF_HUB_OFFLINE=1`; missing files print the exact cache reason and exit 77 before requiring Docker. Corrupt cached files and inference errors fail instead of skipping. The selected model and revision are printed. This mode enables the existing `codeterm-mem/embeddings` feature, so SQLite uses sqlite-vec. It remains a separate run, with the same corpus, queries, config and thresholds:

```bash
bash evals/mem-backend-parity/run.sh --embedder local
```

To use another configured model from the production local provider catalog, pass its model ID, for example:

```bash
bash evals/mem-backend-parity/run.sh --embedder local --local-model BAAI/bge-small-en-v1.5
```

Use the same Docker start/cleanup commands for either mode. Synthetic and local results must be reported separately; neither establishes parity for a different embedder or an untested platform. The harness has no graph relations and makes no graph or temporal-ranking coverage claim.
