Product/CLI requirement (from the user, mid-effort): planner bootstrap work must be modelable as a task WITHOUT fighting the planning-mode gate.

Requirements:
- Task kind "planning" (task.kind, already a free string set via --type) becomes a recognized kind with special gating: `codeterm plan task add --type planning` is allowed while the plan is in mode=planning, WITHOUT --force. Planning tasks exist only to improve GOAL/Spec/Plan/task-graph structure (e.g. a Planner-profile worker lane like this one).
- Execution/review/build tasks (any other kind, including default) remain blocked in planning mode: TaskCreationRequiresApproval stays for them; --force keeps its current deliberate-draft escape hatch.
- `codeterm plan doctor --kind quality`: planning-kind tasks do NOT trigger the tasks_before_approval error; they still get task-quality checks (body etc.). Execution tasks created in planning mode keep erroring.
- Docs update: docs/ORCHESTRATION.md + src-tauri/prompts/orchestration_toolset.md (Planner Profile + planning-method sections) describe the planning kind and the updated authoring order (a planning task may precede approval).
- Rationale: the Orchestrator dispatches a Planner as a task-tracked lane; today that task is either a --force draft (doctor error) or untracked. Planning work deserves first-class task state without weakening the approval gate for execution.
