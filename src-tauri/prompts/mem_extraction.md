Extract people, projects, decisions, and area from this text. Execute the CLI commands below.

SOURCE: id="{id}", title="{title}"

TEXT:
---
{body}
---

STEP 1 — CLASSIFY AREA first (needed for steps below):
Determine the primary project or topic. Use a short lowercase name (rekap, soundcalm, deploy, auth, infra, home).
Run: codeterm mem update {id} --area <area>

STEP 2 — PEOPLE (only named individuals, not roles):
For each person mentioned by name:
  codeterm mem graph create --id person_<name> --entity-type Person --scope global
  codeterm mem graph relate --from {id} --rel mentions --to person_<name>

STEP 3 — PROJECTS (only named projects/repos/products):
For each project mentioned by name:
  codeterm mem graph create --id project_<name> --entity-type Project --scope global
  codeterm mem graph relate --from {id} --rel mentions --to project_<name>

STEP 4 — DECISIONS (explicit "we decided", "let's use", "better to"):
For each clear decision:
  codeterm mem save --kind fact --title "decision: <short>" --tags auto-extracted,decision --area <area> --importance 2 "<what was decided and why>"

STEP 5 — PROBLEMS (explicit "bug in", "doesn't work", "the issue was"):
For each clear problem:
  codeterm mem save --kind learning --title "problem: <short>" --tags auto-extracted,problem --area <area> --importance 2 "<problem and fix if mentioned>"

STEP 6 — Report:
  codeterm mem worker report "N people, M projects, K decisions, area=<area>"

RULES:
- ONLY create graph entities with ids starting with `person_` or `project_`. Nothing else. Categories (rule/preference/etc) are columns on mem_items, not nodes.
- Do NOT create technology entities — technologies are searchable via FTS in the body text.
- Only extract what's clearly stated. Don't infer or guess.
- IDs: lowercase_underscore. "John Smith" → person_john_smith, "Rekap" → project_rekap.
- Skip generic terms without a specific name.

Person detection — require TWO different signal types before creating a person entity:
- Dialogue: "Tima said X", quoted speech
- Action verb: "Tima built/fixed/shipped X"
- Pronoun reference: "Tima... he/his" nearby
- Direct address: "Tima, can you..."
Only when you see 2+ distinct signal types, create a person. Otherwise skip — may be a common word.

- Use canonical names: "Tima" and "Timofey" → person_tima (pick the short form).
- If both variants appear in the text, alias them:
    curl -s -X POST "$CODETERM_API_URL/api/mem/graph/alias" -H "Authorization: Bearer $CODETERM_API_TOKEN" -H "Content-Type: application/json" -d '{"alias":"person_timofey","canonical":"person_tima"}'
- `graph create` is idempotent — safe to call even if entity already exists.
- Decisions must be specific: "use PostgreSQL" is too vague. "Use PgBouncer transaction mode with max_client_conn=400" is good.
- If nothing to extract, report "nothing to extract" and exit.

Execute now.