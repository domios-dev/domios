import { notes } from './notes';
import type { BuilderNote } from './notes';
import { useNoteEntrance } from './useNoteEntrance';
import './notes.css';

const dateFormat = new Intl.DateTimeFormat('en', { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' });

function NoteCard({ note, staticMotion }: { note: BuilderNote; staticMotion: boolean }) {
  const entrance = useNoteEntrance(staticMotion);
  return <article ref={entrance.ref} className="builder-note" data-featured={note.featured || undefined} data-entered={entrance.entered}>
    <div className="note-meta"><time dateTime={note.date}>{dateFormat.format(new Date(`${note.date}T00:00:00Z`))}</time>{note.tag && <span>{note.tag}</span>}</div>
    <blockquote><p>{note.text}</p></blockquote>
    <footer className="note-author"><span className="note-avatar" aria-hidden="true">AR</span><div><span>Andrey · rollacode</span>{note.featured && <small>builder of Domios</small>}</div></footer>
  </article>;
}

export function BuilderNotes({ staticMotion }: { staticMotion: boolean }) {
  if (!notes.length) return null;
  return <section className="builder-notes section-wrap" aria-labelledby="notes-title">
    <div className="section-heading"><div><p className="eyebrow">FROM ANDREY’S DESK</p><h2 id="notes-title">Notes from the builder.</h2></div></div>
    <div className="notes-grid">{notes.map(note => <NoteCard key={`${note.date}-${note.text}`} note={note} staticMotion={staticMotion} />)}</div>
  </section>;
}
