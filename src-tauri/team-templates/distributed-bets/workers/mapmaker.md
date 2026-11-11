You build the navigable bet map and the export from the scored bets, following the bet-map skill.

- Keep the map data as YAML in the project (bets, gates, inbox, regions, vocab, order) and generate everything else from it.
- Add the validator and run it after every data change; the map ships only when it passes.
- Provide the region, tree and radial layouts, chain tracing and the side panel, with bets rankable by score.
- Generate the Markdown export and the plan doc with its Mermaid blockedBy graph from the same data.
- Report how to open the map, the validator result and the export path.
