Extract people, projects, decisions, and area from this text. Execute the CLI commands below.

SOURCE: id="{id}", title="{title}"

TEXT:
---
{body}
---

STEP 0 — DEDUP BEFORE CREATE:
Before emitting ANY new fact / learning, search existing items first:
  codeterm mem search --kind fact --limit 5 "<two or three keywords from the decision>"
  codeterm mem search --kind learning --limit 5 "<keywords from the problem>"
If a clearly similar item already exists, do NOT create a new one. Update the
existing item instead:
  codeterm mem update <existing_id> --body "<merged body including the new detail>"
Emit a single line in your report for each merge: "update existing: id=<existing_id>".
Only create a brand-new fact/learning when the search returns nothing
overlapping. Example:
  - Source says "we bumped worker pool cap from 2 to 3".
  - Search: `codeterm mem search --kind fact --limit 5 "worker pool cap"`.
  - Returns fact_worker_pool_cap with body "cap=2".
  - Action: `codeterm mem update fact_worker_pool_cap --body "cap=3 (bumped from 2)"`.
  - Report line: "update existing: id=fact_worker_pool_cap".

STEP 1 — CLASSIFY AREA (deterministic table, do NOT invent areas):

  body mentions                                                         -> area
  ----------------------------------------------------------------------------
  codeterm / worker / cron / mem / knowledge / graph / dreaming         -> codeterm
  soundcalm / binaura / audio engine / healthkit                        -> soundcalm
  rekap / langgraph / langchain / livekit / whisper                     -> rekap
  home assistant / mosquitto / router / tailscale / 4090              -> home
  (no match)                                                            -> null

Use the FIRST matching area. Do NOT invent new areas. Do NOT guess
"system", "performance", "infrastructure", "misc", or any other label that
is not in the table. If nothing matches, leave area NULL — run:
  codeterm mem update {id} --area ""
(empty string clears the field). Otherwise:
  codeterm mem update {id} --area <area>

STEP 2 — PEOPLE (only named individuals, not roles):
For each person mentioned by name:
  codeterm mem graph create --id person_<name> --entity-type Person --scope global
  codeterm mem graph relate --from {id} --rel mentions --to person_<name>

STEP 3 — PROJECTS (only named projects/repos/products):
For each project mentioned by name:
  codeterm mem graph create --id project_<name> --entity-type Project --scope global
  codeterm mem graph relate --from {id} --rel mentions --to project_<name>

STEP 4 — DECISIONS (explicit "we decided", "let's use", "better to"):
First run STEP 0 dedup search. Then, for each decision not already present:
  codeterm mem save --kind fact --title "decision: <short>" --tags auto-extracted,decision --area <area> --importance 2 "<what was decided and why>"

STEP 5 — PROBLEMS (explicit "bug in", "doesn't work", "the issue was"):
First run STEP 0 dedup search. Then, for each problem not already present:
  codeterm mem save --kind learning --title "problem: <short>" --tags auto-extracted,problem --area <area> --importance 2 "<problem and fix if mentioned>"

STEP 6 — Report:
  codeterm mem worker report "N people, M projects, K decisions, U updates, area=<area>"

RULES:
- ONLY create graph entities with ids starting with `person_` or `project_`. Nothing else. Categories (rule/preference/etc) are columns on mem_items, not nodes.
- Do NOT create technology entities — technologies are searchable via FTS in the body text.
- Only extract what's clearly stated. Don't infer or guess.
- IDs: lowercase_underscore. "John Smith" → person_john_smith, "Rekap" → project_rekap.
- Skip generic terms without a specific name.

HALLUCINATION BAN — this is the hard rule, violating it poisons the store:
- Do NOT add information that is not present in the source text.
- Do NOT write "Verified by <person>", "Confirmed by <person>", "Approved by
  <person>" unless the source text literally says that someone verified,
  confirmed, or approved it.
- Do NOT invent dates, version numbers, commit hashes, ticket IDs, or metric
  values. If the source says "we fixed it last week", write "we fixed it
  recently" — do not pick a specific date.
- Do NOT speculate. No "probably", "likely", "this means that...", "one
  could argue that...". If the source says "we decided X", write "we decided
  X". Full stop.
- Do NOT add confirmations, sign-offs, or attributions that were not
  explicitly stated. If nobody in the source said "LGTM", the fact must not
  say anyone approved it.
- When in doubt, write LESS. A short, accurate fact is worth more than a
  long, embellished one.

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
