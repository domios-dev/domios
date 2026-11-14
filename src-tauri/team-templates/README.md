# Domios Teams

The official team channel for [Domios](https://github.com/domios-dev/domios).
The packages use the [Agentic Team Protocol](https://github.com/domios-dev/agentic-team-protocol)
and are licensed under Apache-2.0 (see LICENSE).

Domios bundles a pinned snapshot as its offline floor. This official channel may
supply newer compatible teams for new starts; running teams keep their starting
version. The channel archive is published at:

`https://github.com/domios-dev/domios-teams/releases/download/domios-teams-latest/domios-teams.tar.gz`

## Publish your own channel

Create a repository with a package folder for each ATP team and this root index:

```json
{
  "schemaVersion": 1,
  "name": "example-teams",
  "displayName": "Example teams",
  "description": "Teams shared by Example",
  "teams": [
    {
      "name": "com.example/research",
      "path": "research",
      "version": "1.0.0",
      "minDomios": "1.20.0"
    }
  ]
}
```

Each folder contains `team.yaml` and all referenced files. The name and version
must match the package. Use your own namespace; `dev.domios` belongs to Domios.
Validate each package with the ATP reference validator before publishing. Keep
package and index versions together and increase the version when content changes.

Publish the repository's default branch, or a gzip tar archive containing
`channel.json` and its package folders at the archive root. For an archive URL,
publish a sibling `<archive>.sha256` containing the SHA-256 and archive filename.
Subscribe with:

```sh
domios team channel add https://github.com/example/teams.git
# Or: domios team channel add https://example.org/teams.tar.gz
```

Community teams show their source and require consent for each exact package.
Community channels refresh only on an explicit owner action; they never auto-update.
Only this official channel is trusted and refreshed automatically.

## Maintainers

See CONTRIBUTING.md for validation and publication. The archive, its checksum and
`domios-teams.source-commit` are rolling assets on `domios-teams-latest`. Publishing
never moves that tag and refuses unrelated history or an older commit replacing a
newer one.
