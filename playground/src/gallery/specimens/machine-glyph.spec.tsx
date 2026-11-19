import { useState } from "react";
import { Check } from "lucide-react";
import { MachineGlyph } from "../../../../src/components/shared/MachineGlyph";
import { MachineIdentityControls, MachineJoinPreview } from "../../../../src/components/MeshPanel/MachineIdentityControls";
import { MachineIdentityEditor } from "../../../../src/components/MeshPanel/MachineIdentityEditor";
import { MACHINE_COLORS, MACHINE_KINDS, machineLabel, type MachineIdentity, type MachineKind } from "../../../../src/lib/machineIdentity";
import { ForcePseudo, PhoneViewport } from "../stageTools";
import { defineSpecimen } from "../types";

const kinds = Object.keys(MACHINE_KINDS) as MachineKind[];
const sizes = ["mark-12", "mark-15", "tile", "rail"] as const;
const cast = [
  { name: "ROG", kind: "desktop", color: "teal", status: "online" },
  { name: "BLACKSTAR", kind: "laptop", color: "blue", status: "active 2h ago" },
  { name: "ubunta", kind: "server", color: "graphite", status: "update required" },
  { name: "pixel", kind: "phone", color: "pink", status: "offline" },
] as const;

type Mode = "palette" | "menu" | "sidebar" | "picker" | "hover" | "focus" | "join" | "expanded" | "editor" | "error" | "light" | "phone";

function Palette() {
  return <div className="grid gap-2">
    {sizes.map(size => <section key={size} aria-label={size} className="grid gap-2">
      <h3>{size}</h3>
      {kinds.map(kind => <div key={kind} className="flex flex-wrap items-center gap-2">
        <span className="w-28">{MACHINE_KINDS[kind].label}</span>
        {MACHINE_COLORS.map(color => <MachineGlyph key={color} kind={kind} color={color} size={size} label={`${color} ${MACHINE_KINDS[kind].word}`} />)}
      </div>)}
    </section>)}
  </div>;
}

function MachineSpecimen({ mode }: { mode: Mode }) {
  const [identity, setIdentity] = useState<MachineIdentity>(cast[0]);
  const [failed, setFailed] = useState(true);
  const preview = (expanded = false) =>
    <MachineJoinPreview name="ROG" value={identity} onChange={setIdentity} initiallyExpanded={expanded} />;
  const choices = <MachineIdentityControls value={identity} onChange={setIdentity} />;

  if (mode === "palette") return <Palette />;
  if (mode === "light") return <div className="rounded p-4" style={{ colorScheme: "light", background: "#f6f8fa", color: "#1f2328" }}><Palette /></div>;
  if (mode === "menu") return <div role="menu" aria-label="Domios hosts" className="ct-menu w-80">
    {cast.map((machine, index) => <button key={machine.name} type="button" role="menuitem" aria-label={machineLabel(machine.name, machine, machine.status)} aria-current={index === 0 || undefined} disabled={index >= 2} className="ct-menu-row ct-machine-menu-row">
      <span className="ct-menu-row__icon"><MachineGlyph {...machine} size="mark-15" /></span>
      <span className="min-w-0 flex-1 truncate">{machine.name}</span>
      <span className="flex shrink-0 items-center gap-1.5 text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]">
        <span aria-hidden="true" className="size-2 rounded-full" style={{ backgroundColor: machine.status === "online" ? "var(--ct-green)" : machine.status === "update required" ? "var(--ct-yellow)" : "var(--ct-muted)" }} />
        {machine.status}
      </span>
      {index === 0 && <Check aria-hidden size={14} className="ct-menu-row__check" />}
    </button>)}
  </div>;
  if (mode === "sidebar") return <div className="ct-card grid w-64 gap-3 p-3">
    {cast.slice(0, 2).map((machine, index) => <div key={machine.name}>
      <div>{index === 0 ? "Atlas" : "Domios"}</div>
      <div className="flex items-baseline gap-1.5 text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]">
        <MachineGlyph {...machine} />{machine.name}{index === 0 && " · working"}
      </div>
    </div>)}
  </div>;
  if (mode === "hover" || mode === "focus") return <ForcePseudo selector='[role="radio"][aria-checked="true"]' state={mode === "hover" ? "hover" : "focus-visible"}><div className="w-80">{choices}</div></ForcePseudo>;
  if (mode === "picker") return <div className="w-80">{choices}</div>;
  if (mode === "phone") return <PhoneViewport><div className="px-3 py-2">{preview(true)}</div></PhoneViewport>;
  if (mode === "join" || mode === "expanded") return <div className="ct-card grid w-80 gap-3 p-3">{preview(mode === "expanded")}<button type="button" className="ct-menu-row">Join</button></div>;
  return <div className="ct-menu w-80"><MachineIdentityEditor name="ROG" value={identity} setValue={setIdentity}
    error={mode === "error" && failed ? "The machine is unreachable." : undefined} retry={() => setFailed(false)} /></div>;
}

export default defineSpecimen({
  id: "machine-glyph",
  title: "Machine identity",
  category: "Mesh",
  where: "src/components/shared/MachineGlyph.tsx",
  stage: "wide",
  Component: MachineSpecimen,
  states: [
    { id: "palette", label: "Kind × colour × size", props: { mode: "palette" } },
    { id: "light", label: "Palette in light", props: { mode: "light" } },
    { id: "menu", label: "Host menu and offline", props: { mode: "menu" } },
    { id: "sidebar", label: "Remote tab and Team", props: { mode: "sidebar" } },
    { id: "picker", label: "Picker selection", props: { mode: "picker" } },
    { id: "hover", label: "Picker hover", props: { mode: "hover" } },
    { id: "focus", label: "Picker focus", props: { mode: "focus" } },
    { id: "join", label: "Join preview", props: { mode: "join" } },
    { id: "expanded", label: "Join choices", props: { mode: "expanded" } },
    { id: "editor", label: "Editor", props: { mode: "editor" } },
    { id: "error", label: "Save failure", props: { mode: "error" } },
    { id: "phone", label: "Phone", props: { mode: "phone" } },
  ],
});
