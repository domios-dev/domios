"""The labelling scheme the board uses. Every label on the board comes from here.

Roles: an agent's label is derived from what it is. The Orchestrator and the
Planner are labelled by their team role. A worker is labelled from its task's
stored executor_role by the first matching family below, evaluated in order.
A role that matches no family is not swallowed: it is labelled Other and the
raw role is shown beside it.
"""

ROLE_FAMILIES = [
    ("Design", ("design",)),
    ("Frontend", ("frontend", "storefront", "electron", "web-ui", "react")),
    ("Backend", ("backend", "rust", "python", "cli", "server", "api", "service", "plan-service", "runtime", "mem", "lane", "orchestration", "lifecycle", "agent", "daemon", "socket", "pty")),
    ("Infrastructure and release", ("infra", "build", "release", "ops", "operations", "deploy", "packaging", "mesh")),
    ("Testing and verification", ("test", "qa", "verif", "e2e", "bench", "eval")),
    ("Review and audit", ("review", "audit", "adversarial")),
    ("Docs", ("docs", "documentation", "writer")),
    ("Full stack", ("fullstack", "full-stack", "chat", "product")),
]
OTHER = "Other"


def role_label(team_role, executor_role):
    """team_role is orchestrator, planner or worker; executor_role is the task's stored field."""
    if team_role == "orchestrator":
        return "Orchestrator", None
    if team_role == "planner":
        return "Planner", None
    r = (executor_role or "").lower()
    if not r:
        return OTHER, "no executor role stored"
    for label, keys in ROLE_FAMILIES:
        if any(k in r for k in keys):
            return label, None
    return OTHER, executor_role


# Event kinds are the milestone and stride kinds the plan store writes. Each is shown under this label; an unknown kind shows as "Note (<kind>)".
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
