import { notes } from './notes';
import type { BuilderNote } from './notes';
import { useNoteEntrance } from './useNoteEntrance';
import { siteLinks } from './siteLinks';
import './notes.css';

const dateFormat = new Intl.DateTimeFormat('en', { day: 'numeric', month: 'short', timeZone: 'UTC' });

function NoteCard({ note, duplicate }: { note: BuilderNote; duplicate: boolean }) {
  return <li className="note-item">
    <article className="builder-note ct-liquid" data-featured={note.featured || undefined} tabIndex={duplicate ? undefined : 0}>
      <header className="note-author">
        <span className="note-avatar" aria-hidden="true">AR</span>
        <a className="note-byline" href={siteLinks.author} target="_blank" rel="noopener noreferrer" tabIndex={duplicate ? -1 : undefined}><span>Andrey</span><span>@rollacode</span></a>
        {note.featured && <span className="note-pin">Pinned</span>}
        <time dateTime={note.date}>{dateFormat.format(new Date(`${note.date}T00:00:00Z`))}</time>
      </header>
      <blockquote><p>{note.text}</p></blockquote>
      {note.tag && <footer className="note-tag">{note.tag}</footer>}
    </article>
  </li>;
}

export function BuilderNotes({ staticMotion }: { staticMotion: boolean }) {
  const feed = useNoteEntrance(staticMotion);
  if (!notes.length) return null;
  return <section className="builder-notes section-wrap" aria-labelledby="notes-title">
    <div className="notes-heading">
      <h2 id="notes-title">From the builder</h2>
      <div className="notes-tools">
        <span className="notes-scroll-hint">Scroll to explore <span aria-hidden="true">↔</span></span>
        {!staticMotion && <button className="notes-motion" type="button" aria-pressed={feed.paused} onClick={feed.toggle}>
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" fill="currentColor">
            {feed.paused ? <path d="M3 1.5 10 6 3 10.5Z" /> : <path d="M3 2h2v8H3zm4 0h2v8H7z" />}
          </svg>
          {feed.paused ? 'Play notes' : 'Pause notes'}
        </button>}
      </div>
    </div>
    <div ref={feed.ref} className="notes-viewport" data-static={staticMotion || feed.paused || undefined}>
      <div className="notes-track">
        {[false, true].map(duplicate => <ul className="notes-list" role="list" aria-hidden={duplicate || undefined} key={String(duplicate)}>
          {notes.map(note => <NoteCard key={`${note.date}-${note.text}`} note={note} duplicate={duplicate} />)}
        </ul>)}
      </div>
    </div>
  </section>;
}
