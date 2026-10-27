# Distribution URL consumers

Source classification at dispatch base `3b4b6982e3b4deb9d5c3b367e8ac3bc6562af723`.
Owner decision for 1.16.1: retain `rollacode/codeterm-releases` for updater,
frontend and core-plugin delivery. Public source, issues, desktop downloads and
the proposed package definitions use `domios-dev/domios`. No mirror cutover,
external repository creation, visibility change or publication is authorized.

## Current source and installation

| Consumer | Action |
| --- | --- |
| `README.md` source clone, release/download links and package hints | Use `domios-dev/domios`, proposed `domios-dev/domios/domios` cask and `domios/domios` Scoop package; mark package repositories as unpublished. |
| `shared/siteLinks.ts` repository link | Use current public source for the UI's repository entry. |
| `packages/{plugin-sdk,chat-engine}/package.json`, SDK README | Point repository metadata to current source; retain existing `@codeterm` package identities. |
| `.github/PULL_REQUEST_TEMPLATE.md` | Current `Repository-Issue` namespace; preserve separate `Public-Issue` mirror references. |
| `scripts/windows-runner-bootstrap.ps1` | Current source clone default, with explicit sequencing grant from Elgar. |
| `docs/release/{public-repo,hosted-arm64}.md`, release skill, CI notification registration example and build peer-setup instructions | Current repository examples; remote policy changes remain owner actions. |
| `tests/catalogue.schema.json` | Current schema identifier URL. |
| API updater refusal and `prod-docs/API-STATE.md` recovery | Current manual desktop download link; this is not signed updater discovery. |
| `codeterm-update/src/checker.rs` package-manager hints | Domios package names, with no channel URL change. |
| `scripts/source-repository.mjs` | Already resolves runtime/override/checkout identity; unchanged. |

## Retained transport and release evidence

| Consumer | Retained contract |
| --- | --- |
| `src-tauri/tauri.conf.json` updater endpoints | Existing current-source endpoint plus mirror fallback, same `domios-latest.json` basename and signing key. |
| `codeterm-update/src/checker.rs`, `channel.rs` | Existing source/mirror repository discovery and immutable release manifest selection; only package command strings change. |
| `codeterm-service-core/src/startup/frontend_bundles.rs`, `frontend-release-contract.json` | `frontend-latest/frontend-manifest.json` on the mirror, separate frontend signature key and immutable release bundles. |
| `codeterm-plugins/src/types.rs`, archive-channel tests | `codeterm-core-latest/codeterm-core.tar.gz` on the mirror; retain checksum/source receipts and independently versioned plugin channel. |
| `.github/workflows/{release*,plugin-release}.yml` | Existing mirror publication, permissions, platform fragments and served readback; no edits in this lane. |
| `scripts/{release-publish,release-promote,release-publish-platform,release-verify-windows,release-dry-run-manifest}.*` | Existing mirror URL assembly and retained artifact/signature/source evidence; no edits in this lane. |
| `scripts/{publish-core-channel,sync-release-notes,release_issue_contract,check-release-closure}.*`, frontend release tooling | Existing plugin channel, notes, public mirror issue namespace and verified closure boundary. |
| `distribution/{scoop,homebrew}` | New desktop URLs point at current source releases. Hash placeholders must be filled from the same accepted run, then compared to anonymous public bytes. |

No fresh signed discovery/readback was executed in this lane: the owner's working
1.16.0 mirror evidence is the stated basis for retention, not a new 1.16.1 proof.
The release owner retains original platform `dry-run-evidence.json`, aggregate
`manifest-local-verification.json`, source SHA, updater signatures, plugin
`.sha256`/`.source-commit`, and public readback receipts. None were supplied here;
their 1.16.1 values and package SHA256s remain UNVERIFIED. This lane neither
rewrites those receipts nor claims that unchanged source proves live delivery.

## Legitimate external, historical and protocol references

| Reference | Classification and treatment |
| --- | --- |
| `rollacode/codeterm-plugins` | Separate external plugin repository/channel, including fixtures and plugin-authoring instructions. Retain. |
| `rollacode/codeterm-firebase`, `rollacode/codeterm-web`, personal profile/author URLs | Independent services or authors; source rename supplies no authority to move them. Retain. |
| `scripts/source-repository-aliases.json` | Historical issue metadata alias interpreted by the current release contract. Retain. |
| Issue references in comments, `evals/*` findings, archived CI run links and JSONL session fixtures | Historical evidence; retain the original repository/issue/run identity. |
| Plugin unreachable-channel fixtures and update-panel error fixtures | Deliberately model an inaccessible old private source; preserve failure coverage. |
| Lane URL normalization/external-reference fixtures | Arbitrary GitHub repository inputs, not current install guidance. Retain. |
| `plugins/provider-codex/src/cloud/transport.ts` and its generated bundle/test | Vendor `wham/environments/by-repo/github/rollacode/codeterm` API route, not a public source link. Elgar explicitly retained it for provider-owner investigation; no protocol behavior changed. |
| Older pinned storage/runner procedures in `build-machine` skill | Historical recipes anchored to explicit old commits/runs; preserve provenance. Current release operations use the release skill and current repository. |

Remaining native/external prerequisites are listed in [the package handoff](README.md).
