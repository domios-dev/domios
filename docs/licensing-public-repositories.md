# Public repository licensing edits awaiting owner approval

Effective date: 2026-10-16. No public changes were performed by this task.

2026-10-16: All versions of this software, including every prior revision in this repository's history, are licensed solely under the PolyForm Shield License 1.0.0. No revision was ever distributed under the MIT License.

Owner decision on 2026-10-16: new versions of `@codeterm/plugin-sdk` and `@codeterm/chat-engine` ship under PolyForm Shield 1.0.0. Already-published npm versions remain licensed under ISC; their published contents and licence grants are unchanged. Each package includes the root LICENSE verbatim in its published files.

Plugins for Domios are not competing products. Authors may write and sell plugins for Domios. This clarification does not authorize selling a competing product based on Domios code.

## Observed public state

GitHub API inventory on 2026-10-16:

| Repository | LICENSE files | README licence statement | Releases |
| --- | --- | --- | --- |
| rollacode/codeterm-releases | None | None; describes private source and public binary mirror | 102 descriptions, no MIT or licence statement found |
| rollacode/codeterm-plugins | None | None | One: transcriber-v0.4.1; no licence statement |
| rollacode/codeterm-thirdparty-channel-demo | None | None | None |

The source repository rollacode/codeterm is private. Other public rollacode repositories in the account inventory are unrelated projects and remain outside this plan.

## Exact proposed content

For codeterm-releases, copy the newly committed root LICENSE and NOTICE byte for byte. It contains the official plain text from https://polyformproject.org/licenses/shield/1.0.0.txt unchanged, followed by:

```text
Required Notice: Copyright (c) 2025 Andrey Kovalev
```

Append these README paragraphs:

```markdown
Licensed under [PolyForm Shield 1.0.0](LICENSE) from 2026-10-16. Required Notice: Copyright (c) 2025 Andrey Kovalev.

2026-10-16: All versions of this software, including every prior revision in this repository's history, are licensed solely under the PolyForm Shield License 1.0.0. No revision was ever distributed under the MIT License.
```

Append this separate paragraph to each existing release description, preserving the existing body:

```text
Licensed under PolyForm Shield 1.0.0 from 2026-10-16
```

## Commands after the owner relays GO

Run from the integrated source checkout in PowerShell. These are proposed commands only; this task did not execute them. Work in a fresh destination directory. Review and merge the resulting PR before updating descriptions.

```powershell
$sourceLicense = (Resolve-Path ./LICENSE).Path
gh repo clone rollacode/codeterm-releases ../codeterm-releases-license
Set-Location ../codeterm-releases-license
git switch -c license/polyform-shield
Copy-Item -LiteralPath $sourceLicense -Destination ./LICENSE
Copy-Item -LiteralPath (Join-Path (Split-Path $sourceLicense) NOTICE) -Destination ./NOTICE
$line = 'Licensed under [PolyForm Shield 1.0.0](LICENSE) from 2026-10-16. Required Notice: Copyright (c) 2025 Andrey Kovalev.'
$notice = [IO.File]::ReadAllText((Join-Path (Get-Location) NOTICE)).Trim()
$readme = [IO.File]::ReadAllText((Resolve-Path ./README.md))
[IO.File]::WriteAllText((Join-Path (Get-Location) README.md), $readme.TrimEnd() + "`n`n" + $line + "`n`n" + $notice + "`n")
git diff --check
git diff
git add LICENSE NOTICE README.md
git commit -m 'docs: declare PolyForm Shield licensing'
git push -u origin license/polyform-shield
gh pr create --repo rollacode/codeterm-releases --title 'docs: declare PolyForm Shield licensing' --body-file ../codeterm-release-license-pr.md
```

Create `../codeterm-release-license-pr.md` containing:

```markdown
Declare PolyForm Shield 1.0.0 licensing from 2026-10-16 and include the copyright holder notice. Existing tags and release assets remain unchanged.

Validation: LICENSE matches the integrated source licence byte for byte; README links to LICENSE.
```

After the approved PR is merged, append notes without changing tags or assets:

```powershell
$note = 'Licensed under PolyForm Shield 1.0.0 from 2026-10-16'
$pages = gh api --paginate --slurp repos/rollacode/codeterm-releases/releases | ConvertFrom-Json
foreach ($page in $pages) {
  foreach ($release in $page) {
    if (($release.body -split "`n") -contains $note) { continue }
    $body = ([string]$release.body).TrimEnd() + "`n`n" + $note + "`n"
    $file = Join-Path (Get-Location) ('release-notes-' + $release.id + '.md')
    [IO.File]::WriteAllText($file, $body)
    gh release edit $release.tag_name --repo rollacode/codeterm-releases --notes-file $file
    if ($LASTEXITCODE -ne 0) { throw "Release edit failed: $($release.tag_name)" }
    $actual = gh api "repos/rollacode/codeterm-releases/releases/$($release.id)" | ConvertFrom-Json
    if ($actual.body -cne $body) { throw "Readback differs: $($release.tag_name)" }
  }
}
```

Keep body backups from the inventory before editing. No command here recreates a tag, changes a target commit, replaces an asset or rewrites history. Existing private/public release-note synchronization can overwrite public-only notes; preserve the licence paragraph when that synchronization is next run, or apply an explicitly approved matching metadata append to the private source releases.

The two public plugin repositories need a separate owner scope decision before applying the same LICENSE and README edits: their source was already public, so this task does not infer that their past grants can be revoked. If approved prospectively, use the same branch/PR procedure per repository, audit their package/manifest fields, and append the note to transcriber-v0.4.1 only after explicit approval. Third-party dependency licences remain intact.
