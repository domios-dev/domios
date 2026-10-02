import { useState } from 'react';
import type { CSSProperties, KeyboardEvent } from 'react';
import { chapterAt, timestamp } from './media';
import type { Trailer } from './media';
import { useMediaLibrary, useVideo } from './useShowcase';
import { Icon } from './Icon';
import { CaptionChips } from './CaptionChips';
import { useMediaQuery } from './usePreferences';
import './showcase.css';

function VideoPlayer({ clip, player }: { clip: Trailer; player: ReturnType<typeof useVideo> }) {
  const [posterFailed, setPosterFailed] = useState(false);
  return <div className="video-player" ref={player.containerRef}>
    <div className="video-stage">
      <div className="video-placeholder"><Icon name="terminal" size={40} /><span>A place for your agents.</span><small>See Domios at work.</small></div>
      {!posterFailed && <img className="video-poster" src={clip.poster.src} alt="Three computers connected through Domios" loading="lazy" width="1280" height="720" onError={() => setPosterFailed(true)} />}
      {!player.failed && <video ref={player.videoRef} muted={!player.soundEnabled} loop={!player.soundEnabled} playsInline preload="none" aria-label="Domios: from your desktop to your whole team" aria-describedby="trailer-description"
        onLoadedMetadata={player.onLoadedMetadata} onLoadedData={player.onLoadedData} onPlay={player.onPlay} onPause={player.onPause} onEnded={player.onEnded} onError={player.onError} onTimeUpdate={player.onTimeUpdate} onSeeked={player.onTimeUpdate}>
        {player.loaded && <><source src={clip.mp4.src} type="video/mp4" /><source src={clip.webm.src} type="video/webm" />{clip.subtitles && <track kind="captions" src={clip.subtitles.src} srcLang="en" label="English" onLoad={(event) => { event.currentTarget.track.mode = 'hidden'; }} />}</>}
      </video>}
    </div>
    <CaptionChips caption={player.loaded ? player.caption : { start: 0, end: clip.duration, text: 'A desktop app for agents. A team across your machines.' }} />
    <div className="video-toolbar"><span><i className="status-dot" />{player.failed ? 'Preview unavailable' : 'Inside Domios'}<span className="video-time" aria-hidden="true">{timestamp(player.time)} / {timestamp(clip.duration)}</span></span>
      {!player.failed && <div className="video-controls">{clip.hasNarration && <button className="sound-control" onClick={player.toggleSound} aria-pressed={player.soundEnabled} aria-label={player.soundEnabled ? 'Mute narration' : 'Turn narration on'}>{player.soundEnabled ? 'Sound on' : 'Sound off'}</button>}<button className="play-control" onClick={player.togglePlayback} aria-label={`${player.playing ? 'Pause' : 'Play'} Domios trailer`}><Icon name={player.playing ? 'pause' : 'play'} size={15} />{player.playing ? 'Pause' : 'Play'}</button></div>}
    </div>
    <details className="transcript" id="trailer-description"><summary>{clip.hasNarration ? 'Read narration transcript' : 'Read video description'}</summary><p>A desktop app opens. Agents work in tabs; a team tracks tasks on a board. Connected computers exchange messages and reports. Providers and plugins add tools. A fleet summary brings the results together.</p>{clip.captions.length > 0 && <ol>{clip.captions.map((caption) => <li key={caption.start}><button onClick={() => player.seek(caption.start)} aria-label={`Play from ${timestamp(caption.start)}: ${caption.text}`}><span>{timestamp(caption.start)}</span>{caption.text}</button></li>)}</ol>}{clip.music && <p className="music-credit">{clip.music.credit} <a href={clip.music.sourcePage}>Source</a> · <a href={clip.music.licenseUrl}>Terms</a></p>}</details>
  </div>;
}

export function Showcase({ reducedMotion }: { reducedMotion: boolean }) {
  const media = useMediaLibrary();
  const chapters = media.chapters;
  const player = useVideo(media, reducedMotion);
  const active = chapterAt(player.time, chapters);
  const [selection, setSelection] = useState({ active, previous: active });
  if (selection.active !== active) setSelection({ active, previous: selection.active });
  const previous = selection.previous;
  const compact = useMediaQuery('(max-width: 800px)');
  const chapter = chapters[active];
  const select = (index: number) => player.seek(chapters[index].start);
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next = index;
    if (event.code === 'ArrowRight' || event.code === 'ArrowDown') next = (index + 1) % chapters.length;
    else if (event.code === 'ArrowLeft' || event.code === 'ArrowUp') next = (index + chapters.length - 1) % chapters.length;
    else if (event.code === 'Home') next = 0;
    else if (event.code === 'End') next = chapters.length - 1;
    else return;
    event.preventDefault();
    select(next);
    document.getElementById(`chapter-${chapters[next].id}`)?.focus({ preventScroll: true });
  };
  return <section id="explore" className="showcase section-wrap" aria-labelledby="showcase-title">
    <div className="section-heading"><div><p className="eyebrow">ONE APP. YOUR WHOLE TEAM.</p><h2 id="showcase-title">Pull up a chair.</h2></div><p>From your desktop to every machine.<br />{Math.round(media.duration)} seconds inside Domios.</p></div>
    <div className="showcase-caption"><div><span className="eyebrow">0{active + 1} / {timestamp(chapter.start)}</span><h3>{chapter.title}</h3></div><p>{chapter.description}</p></div>
    <div className="showcase-workbench">
      <nav className="clip-shelf ct-liquid-segmented" aria-label="Trailer chapters" style={{
        '--sx-from': `${compact ? previous % 2 * 100 : 0}%`, '--sx-to': `${compact ? active % 2 * 100 : 0}%`,
        '--sy-from': `${(compact ? Math.floor(previous / 2) : previous) * 100}%`, '--sy-to': `${(compact ? Math.floor(active / 2) : active) * 100}%`,
      } as CSSProperties}>
        <span aria-hidden="true" key={`${active}-${compact}`} className={`shelf-thumb ${previous !== active ? 'travel' : ''}`} />
        {chapters.map((item, index) => <button key={item.id} id={`chapter-${item.id}`} aria-current={active === index ? 'step' : undefined} aria-controls="trailer-player" aria-label={`${item.label}, play from ${timestamp(item.start)}`} onClick={() => select(index)} onKeyDown={(event) => onKeyDown(event, index)}>
          <span className="clip-still"><img src={`/media/${item.id}-poster.webp`} alt="" loading="lazy" width="480" height="270" /><span className="clip-play"><Icon name="play" size={15} /></span><span className="clip-start">{timestamp(item.start)}</span></span>
          <span className="clip-label"><span className="clip-number">0{index + 1}</span>{item.label}<Icon name="arrow" size={14} /></span>
        </button>)}
      </nav>
      <div className="clip-panel" id="trailer-player"><VideoPlayer clip={media} player={player} /></div>
    </div>
  </section>;
}
