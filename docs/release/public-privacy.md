# Current-tree privacy guard

Run `node scripts/check-public-privacy.mjs` after all release edits, on the exact
committed tree being integrated. Both repository-guard jobs run that command.
The guard detects private IPv4/IPv6 Tailnet addresses, MagicDNS, store endpoints,
home paths, email, hardware addresses, Windows hostnames and captured report text.

The guard inventories `git ls-files --cached -z` and reads current tracked bytes,
including newly staged files. It does not walk history or untracked output.
An empty or oversized inventory, missing file, nonregular entry or unreadable
policy fails closed. Limits are 30,000 files and 512 MiB total; files are read in
64 KiB chunks with a 4 KiB token overlap, including large benchmark timelines.
Exit 0 means no unclassified pattern; 1 means findings; 2 means incomplete scan.
Diagnostics contain file, line and category, never matched values. Findings in a
tracked filename are also checked. This is a bounded pattern guard, not proof
that arbitrary conversation text, encoded data or every possible secret is safe.
Review captures manually and pair it with the existing gitleaks scan.

`scripts/public-privacy-allowlist.json` classifies retained material by exact
file, category and SHA-256 of the matched token, with a reason. It contains no
blanket directory exemptions. Synthetic CGNAT/MagicDNS and private-range URLs
exercise parsing, proxy refusal and listener boundaries; public subnet prefixes
document network constraints. Constructed home paths exercise quoting, case and
peer translation. URL-userinfo rejection strings are not recipient addresses.
Public dependency metadata and deliberately public legal contacts are retained.
Legal contact data must not be edited as part of a scrub. New or changed tokens
require fresh classification; never baseline a real private value to get green.
Reserved example domains, loopback, conventional synthetic home principals and
environment-variable references are recognized directly by the guard.

For a current-tree secret audit, export the exact commit to an isolated directory
with `git archive`, then run gitleaks **inside that directory**:

```text
gitleaks version
gitleaks detect --help
gitleaks detect --no-git --redact --source . --config .gitleaks.toml
```

Use a relative source so anchored allowlist paths resolve relative to the export.
An absolute source can produce absolute finding paths that miss those rules.
Never substitute an unbounded history scan for a missing base. The separate
secret-scan workflow owns its validated commit-range policy.

Record the inspected SHA and tracked count privately. For CI, enumerate a bounded
run set through authenticated `gh`, then identify selected run/job logs and
artifact IDs, reviewed archive members, limits and unavailable evidence. Keep
candidate counts separate from confirmed private findings; hosted runner paths,
synthetic failed-test output and public dependency contacts need classification.
An outer ZIP review does not establish coverage of a nested tar or binary payload.
Do not copy raw logs, personal identities or secrets into reports or this tree.

Hosted log/artifact deletion, retention changes, actual runner isolation and
credential rotation are separate owner operations. An actual secret requires a
private owner incident handoff with location only; stop handling its body.
Current-tree success makes no claim about public history. Do not replace the
repository, change visibility or rewrite history as part of this procedure.
