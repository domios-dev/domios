# Published plan database fixtures

SQL before `PRAGMA user_version` is copied verbatim from the published source
constants named in `manifest.json`. Remaining statements insert synthetic
records described independently in `records.json`. No fixture uses the current
schema creator. Supported v0 uses the legacy DDL shipped in the v1.18.6 converter.
v1.18.5 and v1.18.6 have identical persisted DDL and share a fixture. v1.18.7
represents the incident release after its index/FTS upgrade has already run.

Fingerprints are SHA-256 of compact UTF-8 JSON tuples `(type, name, table, SQL)`
from `sqlite_schema`, ordered by type/name, excluding SQLite internal names.
SQL tokens are joined by NUL, preserving quoted text and token boundaries.
Dump and record digests cover LF UTF-8 bytes; SQL checkouts are pinned to LF.

Published fixture entries, records and the initial migration entry are sealed
by `scripts/check-plan-schema.py`. Append future fixtures and migration entries;
never refresh historical inputs. Schema changes require a new target fixture,
version and migration entry, and an implemented open path that passes every
supported fixture through upgrade and reopen. The 1.18.8 entry records existing
v0 conversion and v1 additive repair without introducing a migration framework.

The Rust contract fingerprints real `create_schema` output. Release tests
exercise real open. The Python guard checks DDL and creation wiring and runs
in release-preflight and both CI check workflows.
