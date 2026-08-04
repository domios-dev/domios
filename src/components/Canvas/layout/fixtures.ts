import type { Milestone, OrchestratorRef, Plan, Section, Task } from "../types";
import type { WorkspaceLayout } from "./types";
import { boxesOverlap } from "./geometry";
import { KNOWN_NODE_KINDS } from "./types";

export function canvas() {
  return { x: null, y: null, collapsed: false };
}

export function owner(id: string): OrchestratorRef {
  return { tab_id: id, session_id: id, title: id };
}

export function section(id: string, subs: Section[] = [], body = "body"): Section {
  return { id, title: id, body, subsections: subs, canvas: canvas() };
}

export function milestone(id: string, kind: string, iteration: number | null = null, links: string[] = []): Milestone {
  return { id, from: owner("o"), kind, text: id, ts: 0, links, loop_id: null, iteration, canvas: canvas() };
}

export function task(id: string, sectionIds: string[], opts: Partial<Task> = {}): Task {
  return {
    id,
    name: id,
    type: "implement",
    model: null,
    model_rationale: null,
    status: "pending",
    status_label: null,
    section_ids: sectionIds,
    body: "",
    intent: null,
    assigned_tab_id: null,
    peer: null,
    parent_task_id: null,
    canvas: canvas(),
    milestones: [],
    created_at: 0,
    ...opts,
  };
}

export function plan(
  id: string,
  ownerId: string,
  updatedAt: number,
  sections: Section[],
  tasks: Task[],
  opts: Partial<Plan> = {},
): Plan {
  return {
    id,
    title: `plan-${id}`,
    sections,
    status: "active",
    owner: owner(ownerId),
    project_path: "/p",
    created_at: 0,
    updated_at: updatedAt,
    milestone_kinds: [
      { key: "status_update", label: "Status", color: null },
      { key: "testing", label: "Testing", color: null },
      { key: "review", label: "Review", color: null },
    ],
    loops: [],
    elements: [],
    tasks,
    canvas: { viewport: null },
    owner_history: [],
    ...opts,
  };
}

const CORE_KINDS = new Set<string>([...KNOWN_NODE_KINDS, "handoff"]);

export function coreSolidNodes(layout: WorkspaceLayout) {
  return layout.frames.flatMap((f) => f.nodes.filter((n) => CORE_KINDS.has(n.kind)));
}

export function layoutFingerprint(layout: WorkspaceLayout) {
  return JSON.stringify({
    frames: layout.frames.map((f) => ({
      planId: f.planId,
      box: f.box,
      anchors: [...f.anchors.entries()].sort(([a], [b]) => a.localeCompare(b)),
      nodes: f.nodes
        .map((n) => ({ id: n.id, kind: n.kind, box: n.box }))
        .sort((a, b) => a.id.localeCompare(b.id)),
    })),
    groups: layout.groups.map((g) => ({ ownerKey: g.ownerKey, box: g.box, planIds: g.planIds })),
    bounds: layout.bounds,
  });
}

export function overlappingPairs<T extends { id?: string; box: { x: number; y: number; width: number; height: number } }>(
  nodes: T[],
  padding = 0,
): [T, T][] {
  const hits: [T, T][] = [];
  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      if (boxesOverlap(nodes[i].box, nodes[j].box, padding)) hits.push([nodes[i], nodes[j]]);
    }
  }
  return hits;
}

export function bandedSeedPlans(): Plan[] {
  const specId = "spec_banded_seed";
  const sections = [
    section("sec_layout", [section("sec_layout_nested")], "Layout + banded task field"),
    section("sec_nav", [], "Trackpad navigation"),
    section("sec_isolation", [], "Workspace scoping"),
    section("sec_inbox", [], "Standalone inbox tasks"),
    section("sec_cli", [], "Task CLI + canvas focus"),
    section("sec_mode", [], "Plan-as-mode composer"),
  ];
  const secIds = ["sec_layout", "sec_layout_nested", "sec_nav", "sec_isolation", "sec_inbox", "sec_cli", "sec_mode"];

  const parent = task("t_parent", ["sec_layout"], {
    status: "in_progress",
    created_at: 10,
    body: "x".repeat(600),
    milestones: [milestone("m1", "status_update"), milestone("m2", "testing"), milestone("m3", "review")],
  });
  const subtasks = [
    task("t_sub_a", [], { parent_task_id: "t_parent", status: "pending", created_at: 11 }),
    task("t_sub_b", [], { parent_task_id: "t_parent", status: "in_progress", created_at: 12 }),
    task("t_sub_c", [], { parent_task_id: "t_parent", status: "done", created_at: 13 }),
  ];

  const bandedTasks: Task[] = [
    parent,
    ...subtasks,
    task("t_nav", ["sec_nav"], { status: "in_progress", created_at: 20, milestones: [milestone("m4", "decision")] }),
    task("t_iso", ["sec_isolation"], { status: "done", created_at: 30 }),
    task("t_inbox", ["sec_inbox"], { status: "pending", created_at: 40 }),
    task("t_cli", ["sec_cli", "sec_layout"], { status: "blocked", created_at: 50 }),
    task("t_mode", ["sec_mode"], { status: "in_review", created_at: 60 }),
    ...Array.from({ length: 12 }, (_, i) =>
      task(`t_bulk_${i}`, [secIds[i % secIds.length]], {
        status: i % 3 === 0 ? "in_progress" : i % 2 ? "done" : "pending",
        created_at: 100 + i,
        body: i % 4 === 0 ? "y".repeat(300 + i * 40) : "",
        milestones: i % 5 === 0 ? [milestone(`mb_${i}`, "testing")] : [],
      }),
    ),
  ];

  const main = plan("pl_banded_main", "orchA", 200, sections, bandedTasks, {
    primary_spec_id: specId,
    elements: [
      { id: "ho_1", kind: "handoff", canvas: canvas(), content: { seq: 1 }, links: [] },
      { id: "ho_2", kind: "handoff", canvas: canvas(), content: { seq: 2 }, links: [] },
    ],
  });

  const sibling = plan(
    "pl_banded_sibling",
    "orchA",
    150,
    [section("sec_sib")],
    [task("t_sib", ["sec_sib"], { status: "pending", created_at: 5 })],
    { primary_spec_id: "spec_sibling" },
  );

  const otherOwner = plan(
    "pl_banded_other",
    "orchB",
    175,
    [section("sec_other"), section("sec_other_b")],
    [
      task("t_other_a", ["sec_other"], { status: "in_progress", created_at: 1 }),
      task("t_other_b", ["sec_other_b"], { status: "done", created_at: 2 }),
      task("t_other_c", ["sec_other", "sec_other_b"], { status: "pending", created_at: 3 }),
    ],
    { primary_spec_id: "spec_other" },
  );

  return [main, sibling, otherOwner];
}
