import { useMemo, useRef, useState } from "react";
import { AnimationInspector, FitBox } from "./AnimationInspector";
import type { Specimen } from "./types";

const modules = import.meta.glob("./specimens/*.spec.tsx", { eager: true }) as Record<string, { default?: Specimen }>;

export const specimens = Object.values(modules)
  .map((module) => module.default)
  .filter((specimen): specimen is Specimen => Boolean(specimen))
  .sort((a, b) => a.title.localeCompare(b.title));

function Preview({ specimen }: { specimen: Specimen }) {
  const Component = specimen.Component;
  return (
    <div className="gallery-preview">
      <FitBox pad={24}>
        <Component {...(specimen.states[0]?.props ?? {})} />
      </FitBox>
    </div>
  );
}

export function GalleryIndex() {
  if (!specimens.length) {
    return <main className="portal-inner gallery-empty"><p className="portal-kicker">Component Gallery</p><h1>No specimens yet</h1><p className="portal-lede">Add a <code>*.spec.tsx</code> file in <code>gallery/specimens</code> to populate this space.</p></main>;
  }
  return (
    <main className="portal-inner">
      <p className="portal-kicker">Component Gallery</p>
      <h1>Motion specimens</h1>
      <p className="portal-lede">Live components, their intentional states, and a frame-by-frame view of the browser motion underneath.</p>
      <div className="portal-grid gallery-grid">
        {specimens.map((specimen) => <a className="portal-card gallery-card" href={`#gallery/${specimen.id}`} key={specimen.id}><Preview specimen={specimen} /><span className="portal-card-tag">{specimen.where}</span><h2>{specimen.title}</h2></a>)}
      </div>
    </main>
  );
}

export function GalleryDetail({ id }: { id: string }) {
  const specimen = useMemo(() => specimens.find((entry) => entry.id === id), [id]);
  const [stateId, setStateId] = useState(() => specimen?.states[0]?.id ?? "");
  const stage = useRef<HTMLDivElement>(null);

  if (!specimen) return <main className="portal-inner gallery-empty"><p className="portal-kicker">Component Gallery</p><h1>Specimen not found</h1><a className="portal-back" href="#gallery">← gallery</a></main>;
  const state = specimen.states.find((entry) => entry.id === stateId) ?? specimen.states[0];
  if (!state) return <main className="portal-inner gallery-empty"><h1>{specimen.title}</h1><p className="portal-lede">This specimen has no states.</p></main>;
  const Component = specimen.Component;
  return (
    <main className="portal-inner gallery-detail">
      <a className="portal-back" href="#gallery">← gallery</a>
      <p className="portal-kicker">{specimen.where}</p>
      <h1>{specimen.title}</h1>
      {specimen.notes && <p className="portal-lede">{specimen.notes}</p>}
      <div className="gallery-state-chips" aria-label="Specimen state">
        {specimen.states.map((entry) => <button key={entry.id} className={entry.id === state.id ? "is-active" : ""} type="button" onClick={() => setStateId(entry.id)}>{entry.label}</button>)}
      </div>
      <section className="gallery-live-stage" aria-label={`${specimen.title} live stage`}>
        <div className="gallery-stage-instance" ref={stage} key={state.id}><Component {...state.props} /></div>
      </section>
      <AnimationInspector specimen={specimen} state={state} stage={stage} />
    </main>
  );
}
