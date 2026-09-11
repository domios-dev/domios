"""The labelling scheme the board uses. Every label on the board comes from here.

Areas: the plan declares one tag facet, `area`, with the closed list below.
A task carries exactly one `area:<value>` tag, set when the task is created;
the plan store's apply_tags clears a facet's old value when a new one is set,
so a task can never carry two. The list is declared once and does not change
between sessions. A task without an area tag is shown as Untagged, never
guessed on the board.

Kinds: the milestone and stride kinds the plan store writes, each shown under
one label; an unknown kind shows as Note with the kind in brackets.
"""

AREA_FACET_KEY = "area"
AREAS = [
    ("design", "Design"),
    ("frontend", "Frontend"),
    ("backend", "Backend"),
    ("infra", "Infrastructure"),
    ("testing", "Testing"),
    ("review", "Review"),
    ("docs", "Docs"),
    ("research", "Research"),
]
AREA_LABEL = dict(AREAS)
UNTAGGED = "Untagged"

# The facet in the product's own compact syntax, the one `plan tag-facets set` accepts: key:label:value=Label|...
AREA_FACET_COMPACT = f"{AREA_FACET_KEY}:Area:" + "|".join(f"{v}={l}" for v, l in AREAS)


def task_area(tags):
    """The task's area from its tags: the value after `area:` if it is on the list, else None."""
    for tag in tags or []:
        key, _, value = tag.partition(":")
        if key == AREA_FACET_KEY and value in AREA_LABEL:
            return value
    return None


# Used only to show what tonight's plan would look like once tagged: it was created before the facet existed.
ROLE_HINTS = [
    ("design", ("design",)),
    ("frontend", ("frontend", "storefront", "electron", "react")),
    ("backend", ("backend", "rust", "python", "cli", "server", "api", "service", "runtime", "mem", "lane", "orchestration", "lifecycle", "agent", "daemon", "socket", "pty")),
    ("infra", ("infra", "build", "release", "ops", "deploy", "packaging", "mesh")),
    ("testing", ("test", "qa", "verif", "e2e", "bench", "eval")),
    ("review", ("review", "audit", "adversarial")),
    ("docs", ("docs", "documentation", "writer")),
]


def area_hint(executor_role):
    r = (executor_role or "").lower()
    for value, keys in ROLE_HINTS:
        if any(k in r for k in keys):
            return value
    return None


KIND_LABELS = {
    "planning": "Plan approved",
    "task_created": "Task added",
    "decision": "Decision",
    "review": "Verdict",
    "worker_report": "Report",
    "gate": "Question to you",
    "status_update": "Status",
    "qa": "QA",
    "bounce": "Bounce",
    "accepted": "Accepted",
    "acceptance": "Accepted",
    "dispatch": "Dispatch",
    "incident": "Incident",
    "evidence": "Evidence",
    "testing": "Testing",
    "verification": "Verification",
    "integration": "Integration",
    "cleanup": "Cleanup",
    "skill": "Skill kept",
    "regression": "Regression gate",
    "retrospective": "Retrospective",
    "note": "Note",
}


def kind_label(kind):
    return KIND_LABELS.get(kind) or f"Note ({kind})"
