import { useId, useState } from 'react';
import type { CSSProperties, KeyboardEvent } from 'react';

const studies = [
  { label: 'A team', title: 'Give the work a shape.', note: 'A plan, a few good agents, and room to think.', image: new URL('../../public/media/board-poster.webp', import.meta.url).href, captions: ['A direction for the whole team', 'A space for each agent', 'The work comes together'] },
  { label: 'A mesh', title: 'Let distance disappear.', note: 'Your machines become places the team can reach.', image: new URL('../../public/media/mesh-poster.webp', import.meta.url).href, captions: ['Your laptop starts the conversation', 'A second home joins the mesh', 'Work travels. Context stays clear.'] },
  { label: 'Your tools', title: 'Keep your own instruments.', note: 'Bring the providers and tools that fit the work.', image: new URL('../../public/media/plugins-poster.webp', import.meta.url).href, captions: ['Intelligence is your choice', 'Tools live beside the work', 'Make this place your own'] },
];

export function CaptionChips({ captions, activeIndex }: { captions: readonly string[]; activeIndex: number }) {
  return <div className="liquid-captions" aria-label="Video caption" aria-live="off">
    <span className="caption-anchor" aria-hidden="true" />
    <p className="liquid-caption" key={`${activeIndex}-${captions[activeIndex]}`}><span className="caption-index" aria-hidden="true">0{activeIndex + 1}</span>{captions[activeIndex]}</p>
  </div>;
}

export function LiquidShowcase() {
  const [selected, setSelected] = useState(0);
  const [previous, setPrevious] = useState(0);
  const [caption, setCaption] = useState(0);
  const id = useId();
  const select = (index: number) => { setPrevious(selected); setSelected(index); setCaption(0); };
  const handleKey = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const keys: Record<string, number> = { ArrowRight: (index + 1) % studies.length, ArrowLeft: (index + studies.length - 1) % studies.length, Home: 0, End: studies.length - 1 };
    const next = keys[event.code];
    if (next === undefined) return;
    event.preventDefault();
    select(next);
    document.getElementById(`${id}-tab-${next}`)?.focus();
  };
  const study = studies[selected];
  return <div className="liquid-showcase">
    <div className="common-channel" role="tablist" aria-label="Explore Domios" style={{ '--rail-count': studies.length, '--rail-index': selected, '--rail-from': `${previous * 100}%`, '--rail-to': `${selected * 100}%` } as CSSProperties}>
      <span aria-hidden="true" key={selected} className={`channel-thumb ${selected !== previous ? 'channel-thumb-travel' : ''}`} />
      {studies.map((item, index) => <button key={item.label} id={`${id}-tab-${index}`} role="tab" aria-controls={`${id}-panel-${index}`} aria-selected={selected === index} tabIndex={selected === index ? 0 : -1} onClick={() => select(index)} onKeyDown={event => handleKey(event, index)}><span>0{index + 1}</span>{item.label}</button>)}
    </div>
    {studies.map((item, index) => <div key={item.label} id={`${id}-panel-${index}`} role="tabpanel" aria-labelledby={`${id}-tab-${index}`} hidden={selected !== index} tabIndex={0}>
      {selected === index && <div className="showcase-sheet">
        <div className="showcase-heading"><h3>{study.title}</h3><p>{study.note}</p></div>
        <div className="showcase-picture"><img src={study.image} alt={`${study.label} view in Domios`} /><div className="caption-dock"><CaptionChips captions={study.captions} activeIndex={caption} /></div></div>
      </div>}
    </div>)}
    <div className="caption-controls"><span className="small-label">CAPTION STUDY / SELECT A MOMENT</span><div role="group" aria-label="Caption moments">{study.captions.map((text, index) => <button key={text} onClick={() => setCaption(index)} aria-label={`Caption ${index + 1}: ${text}`} aria-pressed={caption === index}>0{index + 1}</button>)}</div></div>
  </div>;
}
