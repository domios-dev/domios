import { useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Check, ChevronDown, Folder, Home, Users } from "lucide-react";
import { AnchoredPopover } from "./AnchoredPopover";
import { UI_INPUT } from "./design";
import { SELECT_TRIGGER } from "./DropdownMenu";
import { Segmented, type SegmentedOption } from "./Segmented";

/** Where something lives: the machine's General home, one workspace, or one Team. */
export type Destination = { kind: "general" } | { kind: "workspace"; id: string } | { kind: "team"; id: string };
export type DestinationKind = Destination["kind"];

export interface DestinationWorkspace { id: string; name: string; protected?: boolean }
/** A Team may be unnamed; it then reads as its id. */
export interface DestinationTeam { id: string; name?: string | null; workspace_id?: string | null }

const KIND_LABEL: Record<DestinationKind, string> = { general: "General", workspace: "Workspaces", team: "Teams" };
const OPTION = '[role="option"]:not([aria-disabled="true"])';
const MIN_POPOVER_WIDTH = 280;

function KindIcon({ kind }: { kind: DestinationKind }) {
  const Icon = kind === "general" ? Home : kind === "workspace" ? Folder : Users;
  return <Icon size={13} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />;
}

/** The wire form scoped APIs take: "general", "workspace:<id>" or "team:<id>". */
export function destinationScope(destination: Destination): string {
  return destination.kind === "general" ? "general" : `${destination.kind}:${destination.id}`;
}

export function sameDestination(a: Destination | null, b: Destination | null): boolean {
  if (!a || !b || a.kind !== b.kind) return false;
  return a.kind === "general" || a.id === (b as { id: string }).id;
}

/** One place to send something: General, a workspace or a Team, chosen by kind and then by name, with search. Kinds the
 *  caller cannot accept yet stay visible but disabled. */
export function DestinationPicker({ value, onChange, workspaces, teams, kinds = ["general", "workspace", "team"], disabled, placeholder = "Choose a destination", "aria-label": ariaLabel = "Destination" }: {
  value: Destination | null; onChange: (next: Destination) => void;
  workspaces: readonly DestinationWorkspace[]; teams: readonly DestinationTeam[];
  /** The kinds this destination accepts; the others show disabled. */
  kinds?: readonly DestinationKind[];
  disabled?: boolean; placeholder?: string; "aria-label"?: string;
}) {
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const listId = useId();
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState<DestinationKind>(value?.kind ?? kinds[0] ?? "general");
  const [query, setQuery] = useState("");
  const places = workspaces.filter((workspace) => !workspace.protected);
  const workspaceName = (id: string | null | undefined) => workspaces.find((workspace) => workspace.id === id)?.name;
  const label = !value ? null : value.kind === "general" ? "General"
    : value.kind === "workspace" ? workspaceName(value.id) ?? value.id
    : teams.find((team) => team.id === value.id)?.name || value.id;
  const needle = query.trim().toLowerCase();
  const teamCount = (id: string) => teams.filter((team) => team.workspace_id === id).length;
  const rows: Array<{ destination: Destination; name: string; hint?: string }> = kind === "general" ? [{ destination: { kind: "general" }, name: "General", hint: "This machine's home" }]
    : kind === "workspace" ? places.map((workspace) => ({ destination: { kind: "workspace" as const, id: workspace.id }, name: workspace.name,
      ...(teamCount(workspace.id) > 0 ? { hint: `${teamCount(workspace.id)} ${teamCount(workspace.id) === 1 ? "Team" : "Teams"}` } : {}) }))
    : teams.map((team) => ({ destination: { kind: "team" as const, id: team.id }, name: team.name || team.id, hint: workspaceName(team.workspace_id) }));
  const shown = needle ? rows.filter((row) => `${row.name} ${row.hint ?? ""}`.toLowerCase().includes(needle)) : rows;
  const enabled = kinds.includes(kind);
  const options: SegmentedOption<DestinationKind>[] = (["general", "workspace", "team"] as const).map((entry) => ({ value: entry, label: KIND_LABEL[entry], disabled: !kinds.includes(entry) }));
  const choose = (next: Destination) => { onChange(next); setOpen(false); trigger.current?.focus(); };

  // Opening lands on the current choice, or the first one, so arrows and Enter work at once.
  useLayoutEffect(() => {
    if (!open) return;
    const items = list.current?.querySelectorAll<HTMLElement>(OPTION);
    (list.current?.querySelector<HTMLElement>('[role="option"][aria-selected="true"]') ?? items?.[0])?.focus({ preventScroll: true });
  }, [open, kind]);

  const onListKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(list.current?.querySelectorAll<HTMLElement>(OPTION) ?? []);
    const index = items.indexOf(document.activeElement as HTMLElement);
    const target = event.key === "ArrowDown" ? items[Math.min(items.length - 1, index + 1)] : event.key === "ArrowUp" ? items[Math.max(0, index - 1)]
      : event.key === "Home" ? items[0] : event.key === "End" ? items[items.length - 1] : null;
    if (!target) return;
    event.preventDefault();
    target.focus();
  };

  return <>
    <button ref={trigger} type="button" className={SELECT_TRIGGER} disabled={disabled} aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? listId : undefined}
      aria-label={`${ariaLabel}: ${label ?? placeholder}`} onClick={() => { setKind(value?.kind ?? kinds[0] ?? "general"); setQuery(""); setOpen(!open); }}>
      <span className={`min-w-0 flex-1 truncate text-left ${label ? "" : "text-[var(--ct-muted-fg)]"}`}>{label ?? placeholder}</span>
      <ChevronDown size={12} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />
    </button>
    {/* Anchored under the trigger and never narrower than it. */}
    <AnchoredPopover open={open} anchor={trigger} width={Math.max(MIN_POPOVER_WIDTH, trigger.current?.getBoundingClientRect().width ?? 0)} onClose={() => setOpen(false)}>
      <div role="dialog" aria-label={ariaLabel} className="ct-menu flex flex-col gap-1.5 p-1.5">
        <Segmented<DestinationKind> aria-label={`${ariaLabel} kind`} className="self-start" value={kind} options={options} onChange={(next) => { setKind(next); setQuery(""); }} />
        {kind !== "general" && <input type="search" className={UI_INPUT.fullCompact} aria-label={`Search ${KIND_LABEL[kind].toLowerCase()}`} placeholder={`Search ${KIND_LABEL[kind].toLowerCase()}`}
          value={query} disabled={!enabled} onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => { if (event.key !== "ArrowDown") return; event.preventDefault(); list.current?.querySelector<HTMLElement>(OPTION)?.focus(); }} />}
        <div ref={list} id={listId} role="listbox" aria-label={KIND_LABEL[kind]} className="flex max-h-60 flex-col overflow-y-auto" onKeyDown={onListKey}>
          {shown.length === 0 && <p className="ct-menu-caption">{needle ? `Nothing matches “${query.trim()}”.` : `No ${KIND_LABEL[kind].toLowerCase()} yet.`}</p>}
          {shown.map((row) => {
            const selected = sameDestination(row.destination, value);
            return <div key={row.destination.kind === "general" ? "general" : row.destination.id} role="option" tabIndex={-1} aria-selected={selected} aria-disabled={!enabled || undefined}
              className="ct-menu-row" onClick={() => { if (enabled) choose(row.destination); }}
              onKeyDown={(event) => { if ((event.key === "Enter" || event.key === " ") && enabled) { event.preventDefault(); choose(row.destination); } }}>
              <KindIcon kind={row.destination.kind} />
              <span className="min-w-0 flex-1 truncate">{row.name}</span>
              {row.hint && <span className="ct-menu-row__description min-w-0 max-w-[45%] shrink truncate">{row.hint}</span>}
              {/* A fixed slot keeps the check in one column whether or not a row has a hint. */}
              <span className="flex w-[13px] shrink-0 justify-center">{selected && <Check size={13} aria-hidden="true" className="ct-menu-row__check" />}</span>
            </div>;
          })}
        </div>
      </div>
    </AnchoredPopover>
  </>;
}
