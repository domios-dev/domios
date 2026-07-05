# Spec ⊥ Plan — split SPEC into its own object (design proposal)

<!-- revision: 1 -->

Resolves Andrey's decision: **Spec and Plan are TWO DIFFERENT OBJECTS**, connected by
explicit typed links (`spec_section ↔ plan_section`), rendered as canvas edges
(SPEC ← edges → PLAN ← edges → tasks). The spec exists independently — can predate a
plan, persist alone, link to >1 plan. Supersedes §24.3's "two VIEWS of one section set"
(SPEC/PLAN were the same `Plan.sections` rendered twice). Now they are distinct entities.

Grounded in: `plan/types.rs` (`Plan`, `Section`, `CanvasElement`, universal `links`),
§2 (data model), §17 (externalized `.md` bodies), §26 (worlds W0–W3 content-LOD),
§27 (handoff = element + ghost pattern), §28 (batch write path).

---

## 1. Data model

### 1.1 The Spec entity — reuse `Section`, mirror `Plan`'s storage

```rust
pub struct Spec {
    pub id: String,                 // "sp_<uuid>" — stable across restart, like pl_
    pub title: String,
    pub sections: Vec<Section>,     // REUSE the existing Section type verbatim
    pub status: PlanStatus,         // reuse Active/Archived
    pub project_path: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub owner: OrchestratorRef,     // who authored it (optional)
    pub tag_facets: Vec<TagFacet>,  // §19 facets, same as Plan
    pub links: Vec<String>,         // universal link primitive (§2.6)
    pub canvas: PlanCanvas,
    pub source_path: Option<String>, // §17.5 external-doc origin (a real .md spec on disk)
}
```

`Section { id, title, handle, tags, body, subsections, canvas }` is **already** exactly a
prose/requirements unit — no new type. Spec sections carry the prose `body`; plan sections
become pure coverage anchors (their `body` empties — see migration).

### 1.2 Storage — first-class sibling of plans

```
<project>/.codeterm/specs/<spec_id>.json                         # structure + meta
<project>/.codeterm/specs/<spec_id>/sections/<sec_id>.md          # §17 externalized prose
<project>/.codeterm/specs/<spec_id>/INDEX.md                      # §17.5 manifest (generated)
```

Reuses §17 verbatim: point `body_files.rs` at a `specs/` root, same single-writer +
atomic temp+rename + file-watcher rehydrate + `plan-changed`-style emit. Identity stable
across restart (uuid in filename, like plans). A spec living in its own dir is what makes
"predate / persist / shared by >1 plan" structurally true — its lifecycle is decoupled
from any plan dir.

### 1.3 How Plan references Spec — two layers (coarse edge + fine grid)

The decision needs both an object-level attachment AND a section-level grid, with n:m and
"a spec stays untouched when a new plan links it." Solution: **the dependent (plan) owns
the mapping**, so attaching a 2nd plan never writes the spec → no contention, spec is
link-free.

```rust
// On Plan:
pub spec_refs: Vec<String>,          // coarse: spec_ids this plan draws from (object edge)
pub spec_links: Vec<SpecLink>,       // fine: the section↔section grid

pub struct SpecLink {
    pub spec_id: String,
    pub spec_section_id: String,
    pub plan_section_id: String,
    // direction is semantic (requirement → fulfillment); edge drawn undirected (§2.6)
}
```

- **Coarse** `spec_refs` → drives SPEC-node ↔ PLAN-node header attachment + cardinality.
- **Fine** `spec_links` → drives spec-block ↔ plan-row edges + two-hop coverage (§3).

Cardinality of the section link = **n:m, directional, default-authored 1:1.** One plan
section may satisfy many spec requirements; one spec requirement may be covered by many
plan sections. Migration emits 1:1; authors widen later.

### 1.4 Rejected storage alternatives

- **Spec inside the plan dir** (`plans/<id>/spec.json`). Couples lifecycle to the plan —
  directly contradicts predate/persist/share. **Reject.**
- **Spec = `CanvasElement{kind:"spec"}` with sections in `content: Value`.** Zero new
  entity, but sections-in-a-Value kills §17 `.md` externalization (bodies are *precisely*
  the large text §17 exists for), can't live in its own dir / be shared by >1 plan cleanly,
  and overloads an element meant for lightweight annotations. **Reject for the document;**
  keep `CanvasElement` for handoffs (§27).
- **`plan.spec_id: Option<String>`** as the only reference. One spec per plan, nowhere to
  hang the section grid. Too weak alone — folded into `spec_refs` (a Vec) instead.

---

## 2. Migration — deterministic, lossless

Today every plan `Section` carries BOTH prose `body` (the spec) AND tasks (via
`Task.section_ids`). Split each legacy plan on load (§17.3 style, no backward-compat):

1. **Create one Spec** with a *derived* id `sp_<stable_hash(plan_id)>` (idempotent — the
   daemon migrates on load; re-running must be a no-op, and `Math.random`/`Date::now` are
   banned in the deterministic path). Title = the plan title.
2. **Copy each plan section** (recursively, subsection tree preserved) into the spec.
   Spec section id = `sec_<stable_hash(plan_id, plan_section_id)>` (fresh, deterministic).
   Carry `body`, `tags`, `subsections`, `canvas`. The `.md` body file is **copied**
   `plans/<pid>/sections/<sid>.md` → `specs/<spid>/sections/<spec_sid>.md`.
3. **In the plan**, the section KEEPS its id (so `Task.section_ids` never rewrite), title,
   tags, task linkage — but its `body` is **cleared** (the prose now lives in the spec).
   The plan section is now a pure coverage anchor.
4. **Emit one `SpecLink`** `{spec_id, spec_section_id(fresh), plan_section_id(kept)}` per
   copied section (1:1), plus `plan.spec_refs += [spec_id]`.

Why fresh spec ids but kept plan ids: tasks reference plan section ids — never touch them.
Spec sections are a new object → fresh ids. Reusing the same id on both sides (implicit
1:1) breaks the moment a 2nd plan links the same spec or a section splits — so links are
**always explicit**, never id-equality.

**Dogfood `pl_c8a72a9a` (25 sections w/ bodies):** → one spec with 25 prose sections, 25
`.md` bodies copied, 25 `SpecLink`s 1:1, plan retains 25 coverage rows + all tasks intact,
plan section bodies cleared. Lossless: each body lands in exactly one spec section; every
`Task.section_ids` still resolves. Determinism: same plan in → byte-identical spec out.

---

## 3. Coverage across two objects — two-hop

Today: `section ↔ tasks` (covered iff ≥1 task lists the section; uncovered = ⚠).

Now coverage is a **two-hop** relation, rolling UP:

```
spec_section --(SpecLink)--> plan_section --(Task.section_ids)--> tasks
```

A spec section's state (the new, valuable signal the split unlocks):

| State | Meaning | Glyph/tint |
|-------|---------|-----------|
| **Unlinked** | no `SpecLink` → requirement with no plan at all | red/dashed — *strongest* "uncovered" |
| **Linked, 0 tasks** | planned, no work | amber ⚠ (= today's uncovered) |
| **Linked, tasks not all done** | in progress | ● |
| **Linked, all tasks done** | covered | ✓ |

- The **plan node keeps its existing** section↔task coverage tint, unchanged.
- The **spec node gains a derived** tint computed by following `spec_links`.
- A spec section linked to multiple plan sections: coverage = union of tasks across all
  linked plan sections (done/total); "covered" only when all linked work is done.
- Computed **daemon-side** in the assembled `?full=1` payload (and `spec show`) so CLI and
  canvas agree on one coverage truth.

"Unlinked spec section" is the headline new capability: it surfaces *requirements nobody
planned*, which the merged model literally could not represent.

---

## 4. Optionality / cardinality

| Question | Answer | Default |
|----------|--------|---------|
| Must a Plan have a Spec? | **No** — `spec_refs` empty = spec-less board (inbox, ad-hoc) | new plan = no spec |
| Can a Spec have 0 Plans? | **Yes** — the predate case; renders alone, all sections "unplanned" | — |
| Can a Spec have many Plans? | **Yes** — one product spec, many delivery plans (v2/v3) | — |
| Plan section → many spec sections? | **Yes (n:m)** both directions | migration = 1:1 |

`plan create` → no spec by default; `plan create --spec <id>` opts in. Legacy migration
auto-creates 1 spec + 1:1 links so existing boards look identical post-split.

---

## 5. CLI — new `spec` noun, folds onto the §28 batch path

```
codeterm spec create --title … [--project <path>]            # -> sp_…
codeterm spec show <id> [--json|--markdown]                  # assembled prose (like plan get)
codeterm spec list [--project]
codeterm spec section add|update <id> --title … --body-file f.md   # reuse plan section mutation
codeterm spec files <id>                                     # §17.5 manifest / INDEX.md
codeterm spec link <plan_id> --spec <spec_id> \
    [--map <plan_sid>:<spec_sid> ...]                        # explicit; or auto by handle/title
```

**Linking folds onto §28 batch** — add two ops so a single transaction can upsert a
section AND link it via `$ref`:

```
{op:"attach_spec",  spec:"sp_x"}                                   // coarse spec_refs
{op:"link_spec",    spec:"sp_x", spec_section:"$s", plan_section:"$p"}  // fine SpecLink
```

Generalize the batch target so `upsert_section`/`set_section_body` accept `{plan|spec}` —
one write primitive authors both objects (§28.2 spirit). `agent report` / handoff stay
presets over the same path.

**Native + JS parity (architecture rule #2):** spec CRUD + `link_spec`/`attach_spec` are
capability-shaped; each gains a `parity_test.rs` case asserting native and JS drop-ins
produce identical `Spec` JSON + identical `SpecLink`. Core asks capability, never a
provider/object id. Because Spec is plan-shaped, most paths reuse plan service code.

---

## 6. Canvas

- **SPEC node renders the Spec OBJECT** — its own `sections[].body` (externalized §17),
  NOT a `variant="spec"` re-render of `Plan.sections`. Delete the `SectionNode` spec/plan
  variant fork; SPEC and PLAN are now distinct node components fed by distinct entities.
- **Edges spec-block ↔ plan-row come from `plan.spec_links`** (the typed table), NOT from
  "same section id" anymore. n:m supported (a spec block fans to multiple plan rows).
  Dispatched by edge kind `spec-link` through the universal edge renderer (§2.6). A coarse
  `plan-spec` edge joins the node headers; `spec_links` join the rows.
- **Worlds content-LOD (§26) applies unchanged** to the SPEC node: W0 mass + coverage
  strip, W1 section-title rail + first-line abstracts, **W2 = bodies render** (the §26
  body-visible fix — now sourced from the Spec entity), W3 full prose + pager + rail.
  Footprint fixed across zoom (§26.4).
- **Coverage tint on spec blocks** = the two-hop derivation (§3), drawn as row/block fill
  (§2.7 tint-primary), edges reinforce.
- **Dangling links** (deleted spec, removed section) render as a **GHOST block** — dashed,
  dimmed, "missing requirement" / "unlinked" — same treatment as the §27 handoff ghost and
  the plan-lineage ghost task. The edge keeps an anchor so the break is *visible*, not
  silently dropped. Symmetric: a plan row whose linked spec section vanished shows a "spec
  section deleted" stub. Dangling links are **never auto-pruned** (honest over silent,
  architecture rule #3 spirit); a `spec relink` / repair flow surfaces and resolves them.

---

## 7. Alternatives & recommendation (summary)

**A — Spec = full first-class entity** (own dir/json/`.md`, typed `SpecLink` on plan).
✅ **RECOMMENDED.** Only option that satisfies predate / persist / shared-by->1-plan.
Reuses `Section` + §17 + single-writer + watcher. Cost: new entity, `spec` CLI noun, routes,
watcher target, parity tests — but ~80% is plan code reuse.

**B — Spec = `CanvasElement{kind:"spec"}`.** Fastest, zero entity. Loses §17 `.md`
externalization (the large bodies), can't share/predate cleanly, overloads elements.
**Reject.**

**C — One Plan, sections tagged `kind: spec|plan`** (split *within* the plan). No new
entity, single file. But spec can't predate/outlive the plan or be shared → does NOT
deliver "two different objects." A half-measure = today + a flag. **Reject.**

**Link storage:** typed `SpecLink` list **on the Plan** (dependent owns the mapping; spec
stays write-free when linked) + coarse `spec_refs` for the object edge. Beats a scalar
`plan.spec_id` (too weak — one spec, no grid) and string-encoded universal `links` (fragile
for an n:m section grid).

**Build order:** (1) `Spec` entity + storage reusing §17, (2) batch ops `attach_spec` /
`link_spec` + generalized target, (3) migration on load, (4) canvas un-fork (SPEC = entity,
edges from `spec_links`), (5) parity tests + §26 Playwright body-visibility on the SPEC node.
```
