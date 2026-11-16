# Distribution URL consumers

Owner decision for 1.19: every client update source is `domios-dev/domios`.
App updater manifests, frontend releases and the `codeterm-core` plugin channel
are read and published there; release writes use the workflow `GITHUB_TOKEN`.
1.19 alone also writes `rollacode/codeterm-releases` (the 1.19 transition
mirror) so 1.18.x clients that fall back to it still reach 1.19. Every such
write carries a `1.19 transition mirror` comment and uses `PACKAGE_REPOS_TOKEN`;
`git grep -n "1.19 transition mirror"` lists the sites to delete after 1.19
ships. Public issues stay in `rollacode/codeterm-releases`.

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
| `src-tauri/tauri.conf.json` updater endpoints | One `domios-dev/domios` `releases/latest` endpoint, same `domios-latest.json` basename and signing key. |
| `codeterm-update/src/checker.rs`, `channel.rs` | One update repository, `domios-dev/domios`; a manifest cached from any other repository is never restored. Headless discovery follows the same repository. |
| `codeterm-service-core/src/startup/frontend_bundles.rs`, `frontend-release-contract.json` | `frontend-latest/frontend-manifest.json` on `domios-dev/domios`, separate frontend signature key and immutable release bundles. |
| `codeterm-plugins/src/types.rs`, archive-channel tests | `codeterm-core-latest/codeterm-core.tar.gz` on `domios-dev/domios`; retain checksum/source receipts and independently versioned plugin channel. |
| `.github/workflows/{release*,plugin-release,frontend-release}.yml` | Publish and verify on the source repository with `GITHUB_TOKEN`; platform fragments cite `domios-dev/domios` assets; marked 1.19 transition mirror steps only. |
| `scripts/{release-publish,release-promote,release-publish-platform,release-verify-windows,release-dry-run-manifest,verify-updater-manifest}.*` | Updater URLs and the served-asset check name `domios-dev/domios`; marked 1.19 transition mirror writes only. |
| `scripts/{publish-core-channel,sync-release-notes,release_issue_contract,check-release-closure}.*`, frontend release tooling | Core channel and notes publish to the source; closure cites the source manifest evidence and closes issues in the public issue repository with `PACKAGE_REPOS_TOKEN`. |
| `distribution/{scoop,homebrew}` | New desktop URLs point at current source releases. Hash placeholders must be filled from the same accepted run, then compared to anonymous public bytes. |

The `frontend-latest` and `codeterm-core-latest` discovery releases in
`domios-dev/domios` must stay `latest=false`, so `releases/latest` keeps
resolving to the newest app release.

## Legitimate external, historical and protocol references

| Reference | Classification and treatment |
| --- | --- |
| `domios-dev/domios-plugins` | Separate external plugin repository/channel, including fixtures and plugin-authoring instructions. Retain. |
| `rollacode/codeterm-firebase`, `rollacode/codeterm-web`, personal profile/author URLs | Independent services or authors; source rename supplies no authority to move them. Retain. |
| `scripts/source-repository-aliases.json` | Historical issue metadata alias interpreted by the current release contract. Retain. |
| Issue references in comments, `evals/*` findings, archived CI run links and JSONL session fixtures | Historical evidence; retain the original repository/issue/run identity. |
| Plugin unreachable-channel fixtures and update-panel error fixtures | Deliberately model an inaccessible old private source; preserve failure coverage. |
| Lane URL normalization/external-reference fixtures | Arbitrary GitHub repository inputs, not current install guidance. Retain. |
| `plugins/provider-codex/src/cloud/transport.ts` and its generated bundle/test | Vendor `wham/environments/by-repo/github/rollacode/codeterm` API route, not a public source link. Elgar explicitly retained it for provider-owner investigation; no protocol behavior changed. |
| Older pinned storage/runner procedures in `build-machine` skill | Historical recipes anchored to explicit old commits/runs; preserve provenance. Current release operations use the release skill and current repository. |

Remaining native/external prerequisites are listed in [the package handoff](README.md).
