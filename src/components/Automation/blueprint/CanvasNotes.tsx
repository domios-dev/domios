import type { ReactNode } from "react";
import { StickyNote } from "lucide-react";
import { NOTE_ITEMS, NOTE_TEXT_LINES, type LaidOutNote } from "./layout";

/** Run notices pinned to the canvas corner: holds, plan lock, missing evidence, stop errors and graph updates. */
export function CanvasNotices({ children }: { children?: ReactNode }) {
  return <div className="bp-canvas-notices" onMouseDown={(event) => event.stopPropagation()}>{children}</div>;
}

/** The Blueprint's own note, laid out above its start so it pans, zooms and fits with the graph. */
export function NoteCard({ laid }: { laid: LaidOutNote }) {
  const { note, box } = laid;
  return <aside className="bp-note" aria-label="Blueprint note" style={{ left: box.x, top: box.y, width: box.width, height: box.height }}>
    <p className="bp-note__title"><StickyNote size={13} aria-hidden="true" /><span>{note.title}</span></p>
    {note.text && <p className="bp-note__text" style={{ WebkitLineClamp: NOTE_TEXT_LINES }}>{note.text}</p>}
    {note.items.length > 0 && <ul className="bp-note__items" aria-label="Intents">
      {note.items.slice(0, NOTE_ITEMS).map((item) => <li key={item}>{item}</li>)}
    </ul>}
    {note.facts.length > 0 && <p className="bp-note__facts">{note.facts.map((fact) => <span key={fact}>{fact}</span>)}</p>}
  </aside>;
}
