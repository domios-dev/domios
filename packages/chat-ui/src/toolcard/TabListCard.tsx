import type { ToolEvent } from "@codeterm/chat-core";

interface TabRow {
  id: string;
  title: string;
  status?: string;
  depth: number;
}

interface TabWorkspace {
  name: string;
  rows: TabRow[];
}

function treeLines(result: string): string[] {
  return result.replace(/\r/g, "").split("\n").filter((line) => line && line !== "Output:" && !line.startsWith("Exit code:"));
}

function parseRow(line: string): TabRow | undefined {
  const indent = line.length - line.trimStart().length;
  const match = /^(\S+)\s+(.+)$/.exec(line.trim());
  if (!match) return undefined;
  const [titleAndMeta, description] = match[2].split(" - ", 2);
  const [title, ...metadata] = titleAndMeta.trimEnd().split(/\s{2,}/);
  const status = [...metadata, description?.trim()].filter(Boolean).join(" · ");
  return { id: match[1], title: title.trim(), status: status || undefined, depth: Math.max(0, indent / 2 - 1) };
}

export function parseTabListTree(result: string): TabWorkspace[] {
  const workspaces: TabWorkspace[] = [];
  let workspace: TabWorkspace | undefined;
  for (const line of treeLines(result)) {
    if (!/^\s/.test(line)) {
      workspace = { name: line.trim().split(/\s{2,}/)[0] || "(detached)", rows: [] };
      workspaces.push(workspace);
      continue;
    }
    const row = parseRow(line);
    if (row && workspace) workspace.rows.push(row);
  }
  return workspaces;
}

export function TabListCard({ event }: { event: ToolEvent }) {
  const workspaces = parseTabListTree(event.result ?? "");
  if (event.status === "pending") return <div data-testid="tab-list-pending" className="text-xs" style={{ color: "var(--ct-muted-fg)" }}>Listing tabs…</div>;
  if (workspaces.length === 0) return <div data-testid="tab-list-empty" className="text-xs" style={{ color: "var(--ct-muted-fg)" }}>{event.result?.trim() || "No tabs"}</div>;
  return (
    <div data-testid="tab-list-compact" className="space-y-2">
      {workspaces.map((workspace) => (
        <section key={workspace.name} data-testid="tab-list-workspace">
          <h4 className="mb-0.5 text-xs font-medium" style={{ color: "var(--ct-fg)" }}>{workspace.name}</h4>
          <ul className="space-y-0.5">
            {workspace.rows.map((row) => (
              <li key={row.id} data-testid="tab-list-row" className="flex min-w-0 items-baseline gap-2 text-xs" style={{ paddingLeft: `${row.depth * 12}px` }}>
                <span className="shrink-0 font-mono text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>{row.id}</span>
                <span className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>{row.title}</span>
                {row.status && <span className="ml-auto shrink-0 truncate text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>{row.status}</span>}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
