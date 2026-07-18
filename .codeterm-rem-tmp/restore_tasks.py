#!/usr/bin/env python3
"""Restore todo+review tasks from pl_ghk358 recovery export into pl_as7ty5.
Daemon RPC hangs on plan writes but commits to disk; we fire with long timeouts
and harvest ids from disk. Idempotent: skips tasks/phases already present by name."""
import os, re, subprocess, time, sys, glob, yaml

ROOT = r"D:\Developer\codeterm"
os.chdir(ROOT)
REC = ".codeterm-rem-tmp/pl_ghk358-recovery.md"
PLAN = "pl_as7ty5"
TASKS_DIR = ".codeterm/plans/active/pl_as7ty5/tasks"
PLAN_YAML = ".codeterm/plans/active/pl_as7ty5/plan.yaml"
PARENT_PHASE = "sec_47k6nr"
LOG = ".codeterm-rem-tmp/restore-progress.log"
SHA = "78a67aaf79b07838e39251ebaff501a63bc3321b6f1ebc8cc1085a7a85ef2e07"

def log(m):
    line = f"[{time.strftime('%H:%M:%S')}] {m}"
    print(line, flush=True)
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(line + "\n")

def run(args, timeout=160):
    try:
        p = subprocess.run(["codeterm"] + args, capture_output=True, text=True,
                           timeout=timeout, encoding="utf-8", errors="replace")
        return p.stdout or ""
    except subprocess.TimeoutExpired:
        return ""

# ---- phase map: OLD sec id -> (title, start_pat, end_pat) ----
PHASES = [
 ("sec_yhc5x1","Phase 1 — Land & verify in-flight PTY delivery fix","sec_yhc5x1","sec_7oj4py"),
 ("sec_7oj4py","Phase 2 — PTY audit consolidation & evidence hardening","sec_7oj4py","sec_l7vkwa"),
 ("sec_l7vkwa","Phase 3 — Issue-triage intake protocol","sec_l7vkwa","sec_a3w0yh"),
 ("sec_a3w0yh","Phase 4 — Crash & data-integrity issues (#235 #237 #236 #239)","sec_a3w0yh","sec_7kbewk"),
 ("sec_7kbewk","Phase 5 — Platform & release issues (#203 #209)","sec_7kbewk","sec_jop3mw"),
 ("sec_jop3mw","Phase 6 — Planner/CLI DX & orchestration issues (#230 #231 #232)","sec_jop3mw","sec_asykd3"),
 ("sec_asykd3","Phase 7 — Small CLI/bridge hygiene issues (#233 #234 #238)","sec_asykd3","sec_ewo6v2"),
 ("sec_ewo6v2","Phase 8 — Frontend issue (#185)","sec_ewo6v2","sec_190dqf"),
 ("sec_190dqf","Phase 9 — Cron/skill-loading reliability (#179)","sec_190dqf","sec_ta6h8v"),
 ("sec_ta6h8v","Phase 10 — Closure gate","sec_ta6h8v","sec_pgcm75"),
 ("sec_pgcm75","Phase 1b — Mesh delivery integrity (#240, P0)","sec_pgcm75","sec_oyfez3"),
 ("sec_oyfez3","Phase 11 — Chat UI agent-binding regression (research-first)","sec_oyfez3","sec_sy0gh3"),
 ("sec_sy0gh3","Phase 12 — Architecture refactor lane integration","sec_sy0gh3","sec_docuvs"),
 ("sec_docuvs","Phase 1c — Provider acceptance observability (P0)","sec_docuvs","NEVERMATCH"),
]

REC_TEXT = open(REC, encoding="utf-8").read()

def extract(start, end):
    lines = REC_TEXT.splitlines()
    out, f = [], False
    for ln in lines:
        if re.match(rf"^### {re.escape(start)}\b", ln): f=True; continue
        if f and (re.match(rf"^### {re.escape(end)}\b", ln) or ln.startswith("## 4.") or ln.startswith("## 3. PLAN")): break
        if f: out.append(ln)
    return "\n".join(out).strip()

def plan_yaml_titles():
    return open(PLAN_YAML, encoding="utf-8").read()

def phase_id_by_title(title):
    txt = plan_yaml_titles()
    # match 'title: X' or "title: 'X'" preceded within a few lines by 'id: sec_'
    for m in re.finditer(r"id: (sec_[a-z0-9]+)\n(?:.*\n){0,3}?\s*title: '?" + re.escape(title), txt):
        return m.group(1)
    # simpler: scan sequential
    cur=None
    for ln in txt.splitlines():
        mi=re.search(r"id: (sec_[a-z0-9]+)", ln)
        if mi: cur=mi.group(1)
        mt=re.search(r"title: '?(.+?)'?\s*$", ln)
        if mt and mt.group(1).strip().strip("'")==title: return cur
    return None

# ---- ensure all 14 phases ----
PHASE_MAP = {}
for old, title, s, e in PHASES:
    nid = phase_id_by_title(title)
    if nid:
        PHASE_MAP[old]=nid; log(f"phase exists {old} -> {nid} ({title[:30]})"); continue
    body = extract(s, e)
    log(f"firing phase {title[:40]}")
    run(["plan","section","add",PLAN,"--parent",PARENT_PHASE,"--title",title,"--body",body], timeout=160)
    # poll disk
    for _ in range(30):
        nid = phase_id_by_title(title)
        if nid: break
        time.sleep(3)
    if nid: PHASE_MAP[old]=nid; log(f"phase landed {old} -> {nid}")
    else: PHASE_MAP[old]=None; log(f"PHASE FAILED {old} ({title[:30]}) — task will be created uncovered")

# ---- parse tasks ----
blocks = re.split(r"^### (task_[a-z0-9]+) — ", REC_TEXT, flags=re.M)
# blocks: [pre, id1, body1, id2, body2, ...]
tasks=[]
for i in range(1, len(blocks), 2):
    tid = blocks[i]; content = blocks[i+1]
    header = content.splitlines()[0]
    mstat = re.search(r'"\s*—\s*([A-Z][A-Z_ ]*?)\s*(?:[(·]|$)', header)
    status = (mstat.group(1).strip() if mstat else "PENDING")
    tasks.append((tid, header, content, status))

SKIP = {"DONE","CANCELLED"}

# cleanup: delete any task wrongly created for a DONE/CANCELLED title (async write from a prior run)
DONE_TITLES = []
for _i in range(1, len(blocks), 2):
    _c = blocks[_i+1]; _h = _c.splitlines()[0]
    _ms = re.search(r'"\s*—\s*([A-Z][A-Z_ ]*?)\s*(?:[(·]|$)', _h)
    if _ms and _ms.group(1).strip() in SKIP:
        _mt = re.search(r'"([^"]+)"', _h)
        if _mt: DONE_TITLES.append(_mt.group(1))
def _find_task_id_by_name(name):
    for fp in glob.glob(TASKS_DIR+"/*.yaml"):
        try:
            txt=open(fp,encoding="utf-8").read()
            m=re.search(r"^name:\s*'?\"?(.+?)'?\"?\s*$", txt, re.M)
            if m and m.group(1).strip()==name.strip():
                mi=re.search(r"^id:\s*(task_[a-z0-9]+)", txt, re.M)
                if mi: return mi.group(1)
        except Exception: pass
    return None
for _t in DONE_TITLES:
    # poll briefly in case an async write is mid-flight
    for _ in range(4):
        _id=_find_task_id_by_name(_t)
        if _id:
            log(f"CLEANUP deleting wrongly-created {_id} ({_t[:40]})")
            run(["plan","task","rm",PLAN,_id], timeout=120)
            break
        time.sleep(2)
def norm_status(s):
    s=s.upper()
    if s.startswith("IN_REVIEW") or s.startswith("IN REVIEW"): return "in_review"
    if s.startswith("BLOCKED"): return "blocked"
    if "IN FLIGHT" in s or "IN-FLIGHT" in s or s.startswith("IN_PROGRESS"): return "in_progress"
    return "pending"

def field(content, name):
    # match 'name:' or 'name (final...):' capturing until next 'label:' at line start
    pat = re.compile(rf"^{name}(?: \([^)]*\))?:\s*(.*?)(?=^\w[\w ()]*:\s|\Z)", re.M|re.S)
    m = pat.search(content)
    return m.group(1).strip() if m else ""

def split_criteria(c):
    if not c: return []
    parts = re.split(r";\s*(?=\(\d+\))", c)
    return [re.sub(r"^\(\d+\)\s*","",p).strip() for p in parts if p.strip()]

def existing_task_names():
    names=set()
    for fp in glob.glob(TASKS_DIR+"/*.yaml"):
        try:
            for ln in open(fp,encoding="utf-8"):
                if ln.startswith("name:"):
                    names.add(ln.split(":",1)[1].strip().strip("'\"")); break
        except Exception: pass
    return names

def task_id_by_name(name):
    for fp in glob.glob(TASKS_DIR+"/*.yaml"):
        try:
            txt=open(fp,encoding="utf-8").read()
            m=re.search(r"^name:\s*'?\"?(.+?)'?\"?\s*$", txt, re.M)
            if m and m.group(1).strip()==name.strip():
                mi=re.search(r"^id:\s*(task_[a-z0-9]+)", txt, re.M)
                if mi: return mi.group(1)
        except Exception: pass
    return None

created=0; skipped=0; failed=0; status_set=0
existing=existing_task_names()
summary=[]
for tid, header, content, status in tasks:
    if status in SKIP:
        summary.append((tid,"SKIP-"+status,"")); continue
    title = re.search(r'"([^"]+)"', header).group(1)
    meta = ""
    for ln in content.splitlines():
        if ln.startswith("type:"): meta=ln; break
    def mget(k):
        m=re.search(rf"{k}:\s*([^·]+?)(?:·|$)", meta)
        return m.group(1).strip() if m else ""
    ttype=mget("type") or "feature"
    if ttype=="planning": ttype="planning"
    sec_field=mget("sections")
    old_secs=[s.strip() for s in re.split(r"[, ]+", sec_field) if s.startswith("sec_")]
    new_secs=[PHASE_MAP.get(s) for s in old_secs]
    new_secs=[s for s in new_secs if s]
    model=mget("model") or "claude/sonnet"
    personality=mget("personality")
    erole=mget("executor_role")
    depends=mget("depends_on")
    why=field(content,"why")
    body=field(content,"body")
    criteria=field(content,"criteria")
    testplan=field(content,"test_plan")
    correctness=field(content,"correctness")
    roleg=field(content,"role_guidance")
    # trailing notes (NOTE/WORKER/BLOCK/OUTCOME) fold into body provenance
    notes=[]
    for lab in ["NOTE","WORKER EVIDENCE","WORKER STATE","BLOCK REASON","OUTCOME"]:
        mv=re.search(rf"^{lab}[^\n:]*:\s*(.+?)(?=\n###|\Z)", content, re.M|re.S)
        if mv: notes.append(f"{lab}: {mv.group(1).strip()}")
    prov=f"[RECOVERED from pl_ghk358 as {tid}; export sha256 {SHA[:12]}…"
    if depends: prov+=f"; original depends_on: {depends}"
    if not new_secs and old_secs: prov+=f"; intended phase(s): {','.join(old_secs)} (uncovered — phase section missing)"
    prov+="]"
    full_body=prov+"\n\n"+body
    if notes: full_body+="\n\n"+"\n".join(notes)

    if title in existing or task_id_by_name(title):
        skipped+=1; summary.append((tid,"exists",title[:40])); log(f"skip existing: {title[:45]}"); continue

    args=["plan","task","add",PLAN,"--name",title,"--type",ttype,"--model",model,
          "--why",why or "recovered","--body",full_body,"--correctness",correctness or "recovered",
          "--test-plan",testplan or "see body"]
    for c in split_criteria(criteria) or [criteria or "recovered"]:
        args+=["--criterion",c]
    if roleg: args+=["--role-guidance",roleg]
    if erole: args+=["--executor-role",erole]
    if personality: args+=["--personality",personality]
    if new_secs: args+=["--sections",",".join(new_secs)]
    log(f"creating {tid}: {title[:45]} (status={status})")
    run(args, timeout=170)
    nid=None
    for _ in range(25):
        nid=task_id_by_name(title)
        if nid: break
        time.sleep(3)
    if not nid:
        failed+=1; summary.append((tid,"FAILED",title[:40])); log(f"FAILED to land: {title[:45]}"); continue
    created+=1; log(f"landed {tid} -> {nid}")
    ns=norm_status(status)
    if ns!="pending":
        run(["plan","task","update",PLAN,nid,"--status",ns], timeout=120)
        status_set+=1; log(f"status {nid} -> {ns}")
    summary.append((tid,"CREATED->"+nid,f"{title[:35]} [{ns}]"))

log(f"=== DONE created={created} skipped={skipped} failed={failed} status_set={status_set} ===")
for t in summary: log("  "+str(t))
