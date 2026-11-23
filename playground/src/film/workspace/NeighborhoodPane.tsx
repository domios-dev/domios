import { useId, useState } from "react";
import { Lock, Mail, Plus } from "lucide-react";
import { Button } from "../../../../src/components/shared/Button";
import { FormDialog } from "../../../../src/components/shared/FormDialog";
import ines from "../assets/neighbors/ines.jpg";
import kenji from "../assets/neighbors/kenji.jpg";
import maya from "../assets/neighbors/maya.jpg";
import theo from "../assets/neighbors/theo.jpg";

const neighbors = [
  { name: "Maya", avatar: maya, x: 50, y: 15, detail: "Design review" },
  { name: "Ines", avatar: ines, x: 18, y: 39, detail: "Release notes" },
  { name: "Theo", avatar: theo, x: 82, y: 39, detail: "Sync QA" },
  { name: "Kenji", avatar: kenji, x: 20, y: 72, detail: "Localization" },
];
const add = { x: 50, y: 83 };
const invite = { x: 80, y: 72 };
const position = ({ x, y }: { x: number; y: number }) => ({ left: `${x}%`, top: `${y}%` });

export function NeighborhoodPane({ adding, invited, onAdd, onCloseAdd, onInvite }: {
  adding: boolean; invited: string | null; onAdd: () => void; onCloseAdd: () => void; onInvite: (email: string) => void;
}) {
  const title = useId();
  const [selected, setSelected] = useState<(typeof neighbors)[number] | null>(null);

  return <div className="demo-neighborhood">
    <header className="ct-section-toolbar flex items-center gap-3 px-4">
      <strong>Neighborhood</strong>
      <span className="text-xs text-[var(--ct-muted-fg)]">{neighbors.length} neighbors{invited ? " · 1 invited" : ""}</span>
    </header>
    <div className="demo-neighborhood-stage" role="group" aria-label="Your neighborhood">
      <div className="demo-neighborhood-map">
        <svg className="demo-neighborhood-links" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          {neighbors.map(({ name, x, y }) => <line key={name} x1="50" y1="47" x2={x} y2={y} vectorEffect="non-scaling-stroke" />)}
          {[add, ...(invited ? [invite] : [])].map(({ x, y }) =>
            <line key={x} x1="50" y1="47" x2={x} y2={y} vectorEffect="non-scaling-stroke" className="demo-neighborhood-link--dashed" />)}
        </svg>
        {neighbors.map((neighbor) =>
          <button key={neighbor.name} type="button" className="demo-neighborhood-node" style={position(neighbor)}
            onClick={() => setSelected(neighbor)} aria-label={`${neighbor.name}, connected`}>
            <img src={neighbor.avatar} alt="" className="demo-neighborhood-avatar" />
            <strong>{neighbor.name}</strong>
            <span className="demo-neighborhood-state"><i />Connected</span>
          </button>)}
        <div className="demo-neighborhood-node" style={position({ x: 50, y: 47 })}>
          <span className="demo-neighborhood-avatar demo-neighborhood-self" aria-hidden="true">SA</span>
          <strong>You</strong>
        </div>
        <button type="button" className="demo-neighborhood-node demo-neighborhood-add" style={position(add)}
          data-testid="demo-add-neighbor" onClick={onAdd}>
          <span className="demo-neighborhood-avatar"><Plus size={19} aria-hidden="true" /></span>
          <strong>Add Neighbor</strong>
        </button>
        {invited && <div role="status" aria-label={`Invitation pending for ${invited}`} title={invited}
          className="demo-neighborhood-node demo-neighborhood-invite" style={position(invite)}>
          <span className="demo-neighborhood-avatar"><Mail size={16} aria-hidden="true" /></span>
          <strong>Invited</strong>
        </div>}
      </div>
    </div>
    <p className="demo-neighborhood-privacy"><Lock size={14} aria-hidden="true" /> Neighborhood messages in Domios are end-to-end encrypted.</p>
    {(selected || adding) && <FormDialog titleId={title} onCancel={adding ? onCloseAdd : () => setSelected(null)}
      onSubmit={(event) => {
        event.preventDefault();
        if (adding) onInvite(String(new FormData(event.currentTarget).get("email")));
      }}>
      <h2 id={title} className="font-semibold">{adding ? "Add Neighbor" : selected?.name}</h2>
      {adding ? <>
        <label className="flex flex-col gap-2 text-sm">Email
          <input className="rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2" name="email" type="email" defaultValue="rowan@example.test" required />
        </label>
        <Button type="submit" size="sm" variant="primary">Send sample invite</Button>
      </> : <>
        <p className="text-sm">Connected · {selected?.detail}</p>
        <Button size="sm" onClick={() => setSelected(null)}>Close</Button>
      </>}
    </FormDialog>}
  </div>;
}
