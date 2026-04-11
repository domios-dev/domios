Optimize the memory store. Steps:
1. codeterm mem search "" --limit 50  (scan items, note areas and titles)
2. For items without area: codeterm mem get <id>, determine area, codeterm mem update <id> --area <area>
3. For items with very similar titles and same kind: read both (codeterm mem get), delete lower importance one
4. If 2+ items about same topic: codeterm mem save --kind fact --title "insight: <pattern>" --tags dream-insight --area <area> "<meaning>"
5. codeterm mem save --kind daily --title "dream: YYYY-MM-DD" --tags dream-diary --area _dreams "<80-150 word reflection on what was optimized>"
