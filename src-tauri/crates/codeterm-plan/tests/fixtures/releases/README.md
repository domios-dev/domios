# Plan database release and migration contracts

The published v1.18.5/6, v1.18.7 and supported v0 SQL inputs come from the
literal released sources recorded in `manifest.json`. Synthetic records have
an independent oracle in `records.json`. Never regenerate historical inputs
from the current schema. Equivalent published releases share one fixture.

Numbered SQL steps live in `src/service_plan/store_db/migrations/`, registered
in `migrations.rs`. The first step is the frozen published v1.18.6 row schema.
The v1 bridge admits that shape or the published v1.18.7 shape, retires FTS,
adds the known later indexes and establishes the plan application identity.
Later steps add individual persisted schema changes. Fresh storage runs the
same steps. The v0 converter uses the baseline within the migration owner's
transaction; its backup is taken from a separate reader while that owner
holds the write reservation. It retains malformed-record quarantine semantics.

Every future persisted schema change needs a new step and contiguous version,
an independent schema assertion, a populated forward fixture, and entries in
`manifest.json` and `contract.json`. Constraint steps declare SQL preconditions
whose true result refuses migration before DDL. Published steps, schema
assertions, fixtures and contract entries are immutable. Append entries; never
refresh them. The initial hotfix contract remains sealed as historical evidence.
The target version derives from the last registered step. Schema assertions
are only comparison inputs; production creation executes the numbered chain.

Fingerprints hash compact UTF-8 JSON `(type, name, table, SQL)` tuples from
`sqlite_schema`, sorted by type/name and excluding SQLite internal names.
SQL tokens are NUL-separated, preserving quoted text and token boundaries.
Dump and record digests cover LF UTF-8 bytes. Forward fixtures also pin the
application identity separately from the schema fingerprint.

`python scripts/check-plan-schema.py` checks immutable inputs, the executable
registry, each step's exact schema and application identity, and the independent
current-schema assertion. A fixture update cannot replace a migration step.
The guard runs in existing release-preflight and CI check workflows. Rust
`release_upgrade_tests` and `migrations::tests` exercise real open, exact records,
reopen, supported-source convergence, refusal and transaction rollback.
