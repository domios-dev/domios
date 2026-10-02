import { useState } from 'react';
import type { CSSProperties, KeyboardEvent } from 'react';
import { chapters } from './media';
import type { Clip } from './media';
import { useMediaLibrary, useVideo } from './useShowcase';
import { Icon } from './Icon';
import { CaptionChips } from './CaptionChips';
import { useMediaQuery } from './usePreferences';
import './showcase.css';

function VideoPlayer({ clip, reducedMotion, label }: { clip: Clip; reducedMotion: boolean; label: string }) {
  const player = useVideo(clip, reducedMotion);
  const [posterFailed, setPosterFailed] = useState(false);
  return <div className="video-player" ref={player.containerRef}>
    <div className="video-stage">
      <div className="video-placeholder"><Icon name="terminal" size={40} /><span>{label}</span><small>See your agents at work.</small></div>
      {!posterFailed && <img className="video-poster" src={clip.poster.src} alt={`${label} in Domios`} loading="lazy" onError={() => setPosterFailed(true)} />}
      {!player.failed && <video ref={player.videoRef} muted loop playsInline preload="none" aria-label={`${label} product demonstration`}
        onLoadedData={player.onLoadedData} onPlay={player.onPlay} onPause={player.onPause} onError={player.onError} onTimeUpdate={player.onTimeUpdate} onSeeked={player.onTimeUpdate}>
        {player.loaded && <><source src={clip.mp4.src} type="video/mp4" /><source src={clip.webm.src} type="video/webm" onError={player.onError} /></>}
      </video>}
      <CaptionChips caption={player.caption} />
    </div>
    <div className="video-toolbar"><span><i className="status-dot" />{player.failed ? 'Preview coming soon' : 'A look inside Domios'}</span>
      {!player.failed && <button className="play-control" onClick={player.togglePlayback} aria-label={`${player.playing ? 'Pause' : 'Play'} ${label} video`}><Icon name={player.playing ? 'pause' : 'play'} size={15} />{player.playing ? 'Pause' : 'Play'}</button>}
    </div>
    {clip.captions.length > 0 && <details className="transcript"><summary>Read video description</summary><ol>{clip.captions.map((caption) => <li key={`${caption.start}-${caption.text}`}>{caption.text}</li>)}</ol></details>}
  </div>;
}

export function Showcase({ reducedMotion }: { reducedMotion: boolean }) {
  const [active, setActive] = useState(0);
  const [previous, setPrevious] = useState(0);
  const compact = useMediaQuery('(max-width: 800px)');
  const media = useMediaLibrary();
  const chapter = chapters[active];
  const select = (index: number) => { setPrevious(active); setActive(index); };
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next = index;
    if (event.code === 'ArrowRight' || event.code === 'ArrowDown') next = (index + 1) % chapters.length;
    else if (event.code === 'ArrowLeft' || event.code === 'ArrowUp') next = (index + chapters.length - 1) % chapters.length;
    else if (event.code === 'Home') next = 0;
    else if (event.code === 'End') next = chapters.length - 1;
    else return;
    event.preventDefault();
    select(next);
    document.getElementById(`tab-${chapters[next].id}`)?.focus();
  };
  return <section id="explore" className="showcase section-wrap" aria-labelledby="showcase-title">
    <div className="section-heading"><div><p className="eyebrow">FOUR WINDOWS INTO DOMIOS</p><h2 id="showcase-title">Pull up a chair.</h2></div><p>Pick a window. See how the work moves.</p></div>
    <div className="showcase-workbench">
    <div className="clip-shelf ct-liquid-segmented" role="tablist" aria-label="Product demonstrations" aria-orientation={compact ? 'horizontal' : 'vertical'} style={{
      '--sx-from': `${compact ? previous % 2 * 100 : 0}%`, '--sx-to': `${compact ? active % 2 * 100 : 0}%`,
      '--sy-from': `${(compact ? Math.floor(previous / 2) : previous) * 100}%`, '--sy-to': `${(compact ? Math.floor(active / 2) : active) * 100}%`,
    } as CSSProperties}>
      <span aria-hidden="true" key={`${active}-${compact}`} className={`shelf-thumb ${previous !== active ? 'travel' : ''}`} />
      {chapters.map((item, index) => <button key={item.id} id={`tab-${item.id}`} role="tab" aria-selected={active === index} aria-controls={`panel-${item.id}`} tabIndex={active === index ? 0 : -1} onClick={() => select(index)} onKeyDown={(event) => onKeyDown(event, index)}>
        <span className="clip-still"><img src={media[item.id].poster.src} alt="" loading="lazy" /><span className="clip-play"><Icon name="play" size={15} /></span></span>
        <span className="clip-label"><span className="clip-number">0{index + 1}</span>{item.label}<Icon name="arrow" size={14} /></span>
      </button>)}
    </div>
    {chapters.filter((item) => item.id !== chapter.id).map((item) => <div key={item.id} role="tabpanel" id={`panel-${item.id}`} aria-labelledby={`tab-${item.id}`} hidden />)}
    <div className="clip-panel" role="tabpanel" id={`panel-${chapter.id}`} aria-labelledby={`tab-${chapter.id}`} tabIndex={0}>
      <div className="showcase-caption"><div><span className="eyebrow">{chapter.tag}</span><h3>{chapter.title}</h3></div><p>{chapter.description}</p></div>
      <VideoPlayer key={chapter.id} clip={media[chapter.id]} reducedMotion={reducedMotion} label={chapter.label} />
    </div>
    </div>
  </section>;
}
