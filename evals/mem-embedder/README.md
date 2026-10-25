# Memory embedder evaluation

Retains the global embedder comparison, independent reference oracle and throughput
measurements introduced in `332b96436b`. Read [PREREG-embedder.md](PREREG-embedder.md),
[INTERPRETATIONS.md](INTERPRETATIONS.md) and [DECISION.md](DECISION.md) first. The final
owner decision kept L12 and dropped snapshot recall and new subject sessions;
their absence must not be represented as a passing measurement.

## Preparation

Use an isolated evaluation checkout and disposable stores. Never open production
memory, connect the harness to a live daemon or copy private item text into Git.
Compilation follows the repository's build/gates procedures in an authorized Team
utility shell. The Rust oracle uses the real `codeterm-mem` global provider.

The Python reference needs Python 3, PyTorch and Transformers in a dedicated local
environment. Populate the immutable model revisions listed in `oracle.py` before
running offline. Its `--cache` argument is the Hugging Face hub directory containing
`models--<owner>--<model>/snapshots/<revision>/`; use a local path appropriate to the
machine, not the historical author's cache path. Run measurements on an idle host.

## Reference and throughput commands

From the repository root, build the standalone non-published oracle:

```bash
cargo build --manifest-path evals/mem-embedder/Cargo.toml --release
```

Use its resulting `mem-embedder-oracle` executable (`.exe` on Windows). Explicit
paths keep model-cache and output ownership visible:

```bash
python evals/mem-embedder/oracle.py --binary <oracle-executable> \
  --cache <populated-hub-directory> --out <private-run-directory>/oracle.json
python evals/mem-embedder/throughput.py --binary <oracle-executable> \
  --out <private-run-directory>/throughput.json
```

Create the output directory first. `oracle.py --resume` continues an interrupted
reference comparison, retaining successful model rows; use it only for the same
binary, model revisions and inputs. The oracle loads reference models with
`local_files_only=True`. Keep `HF_HUB_OFFLINE=1` for the production provider too.
Do not overwrite committed historical results when repeating a measurement.

Blueprint retrieval measurements use the procedure in
[blueprint-recall](../blueprint-recall/README.md). Historical `MEM_EVAL_MODEL`
commands in the preregistration refer to scratch-only adapters, not a supported
runtime switch in the delivered application. New provider selection or sealed
query confirmation needs a fresh authorized experiment, not a rerun of old b4.

## Evidence and verdict

The reference output records model revisions, artifact hashes and sizes, binary
digest, per-case cosine values and passage/query results. Each required case must
meet cosine ≥ 0.999; the script stops before recall measurements on a mismatch.
Throughput uses the production binary, one warm-up per model and three repetitions
of its fixed public corpus; report its measured samples and elapsed time separately
from compile time and reference-model timings.

Preserve raw output, command, source/binary identity and machine details. Missing
weights, failed subprocesses, incomplete candidates or nonzero exits are not a
selection result. A new comparison does not change the shipped default. The retained
decision and confirmation artifacts describe their historical runs, not current-head
qualification. No model download or private snapshot access is needed for the
documentation-presence check.
