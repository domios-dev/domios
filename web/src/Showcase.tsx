import { useState } from 'react';
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent } from 'react';
import { chapterAt, timestamp } from './media';
import type { Trailer } from './media';
import { useMediaLibrary, useVideo } from './useShowcase';
import { usePlayerChrome } from './usePlayerChrome';
import { Icon } from './Icon';
import { WordCaptions } from './WordCaptions';
import { useMediaQuery } from './usePreferences';
import './showcase.css';

const trailerDescription = 'A desktop app opens. Agents work in tabs; a team tracks tasks on a board. Connected computers exchange messages and reports. Providers and plugins add tools. A fleet summary brings the results together.';

function VideoPlayer({ clip, player }: { clip: Trailer; player: ReturnType<typeof useVideo> }) {
  const [posterFailed, setPosterFailed] = useState(false);
  const chrome = usePlayerChrome(player.playing, player.failed);
  const narration = clip.captions.map((caption) => caption.text).join(' ');
  const onSurfaceClick = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (player.playing || player.failed || !(event.target instanceof Element)) return;
    if (event.target.closest('.video-rail, .video-error, .fullscreen-error, button, a, input, select, textarea, [role="button"]')) return;
    player.togglePlayback();
  };
  return <div className="video-player" ref={player.containerRef}>
    <div className="video-stage" ref={chrome.stageRef} data-chrome-visible={chrome.visible}
      onPointerEnter={chrome.onPointerEnter} onPointerMove={chrome.onPointerMove} onPointerLeave={chrome.onPointerLeave}
      onPointerDown={chrome.onPointerDown} onFocusCapture={chrome.onFocusCapture} onBlurCapture={chrome.onBlurCapture}
      onClick={onSurfaceClick}>
      <div className="video-placeholder"><Icon name="terminal" size={40} /><span>A place for your agents.</span><small>See Domios at work.</small></div>
      {!posterFailed && <img className="video-poster" src={clip.poster.src} alt="Three computers connected through Domios" loading="lazy" width="1280" height="720" onError={() => setPosterFailed(true)} />}
      {!player.failed && <video ref={player.videoRef} muted={!player.soundEnabled} loop={player.loopEnabled} playsInline preload="none" aria-label="Domios: from your desktop to your whole team" aria-describedby={`trailer-description${clip.hasNarration ? ' trailer-narration' : ''}`}
        onLoadedMetadata={player.onLoadedMetadata} onLoadedData={player.onLoadedData} onPlay={player.onPlay} onPause={player.onPause} onEnded={player.onEnded} onError={player.onError} onTimeUpdate={player.onTimeUpdate} onSeeked={player.onTimeUpdate}>
        {player.loaded && <><source src={clip.mp4.src} type="video/mp4" /><source src={clip.webm.src} type="video/webm" />{clip.subtitles && <track kind="captions" src={clip.subtitles.src} srcLang="en" label="English" onLoad={(event) => { event.currentTarget.track.mode = 'hidden'; }} />}</>}
      </video>}
      <WordCaptions caption={player.loaded && !player.failed ? player.caption : undefined} time={player.time} />
      <div className="video-rail">
        <span className="video-time">{timestamp(player.time)} / {timestamp(clip.duration)}</span>
        <div className="video-controls" role="group" aria-label="Trailer controls">
          {!player.failed && <button type="button" className="play-control" onClick={player.togglePlayback} aria-label={player.playing ? 'Pause Domios trailer' : 'Play Domios trailer'}>
            <Icon name={player.playing ? 'pause' : 'play'} size={17} />
          </button>}
          {!player.failed && clip.hasNarration && <div className="sound-widget"><button type="button" className="sound-control" onClick={player.toggleSound} aria-pressed={player.soundEnabled} aria-label={player.soundAtFullVolume ? 'Turn sound off' : player.soundEnabled ? 'Increase volume to 100%' : 'Turn sound on'}>
            <svg aria-hidden="true" focusable="false" viewBox="0 0 20 20" width="18" height="18"><path d="M3 8v4h3l4 3V5L6 8H3" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />{player.soundEnabled ? <path d="M13 7a4 4 0 0 1 0 6m2-8a7 7 0 0 1 0 10" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /> : <path d="m13 8 4 4m0-4-4 4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />}</svg>
          </button><input className="sound-slider" type="range" min="0" max="100" step="1" value={Math.round(player.volume * 100)} onChange={event => player.setVolume(Number(event.currentTarget.value))} aria-label="Volume" aria-valuetext={`${Math.round(player.volume * 100)}%`} /></div>}
          {chrome.fullscreenSupported && <button type="button" className="fullscreen-control" onClick={chrome.toggleFullscreen} aria-label={chrome.fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}>
            <svg aria-hidden="true" focusable="false" viewBox="0 0 20 20" width="17" height="17"><path d={chrome.fullscreen ? 'M7 3v4H3m14 0h-4V3m0 14v-4h4M3 13h4v4' : 'M3 7V3h4m6 0h4v4m0 6v4h-4M7 17H3v-4'} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /></svg>
          </button>}
        </div>
      </div>
      <p className="visually-hidden" id="trailer-description">{trailerDescription}</p>
      {clip.hasNarration && <p className="visually-hidden" id="trailer-narration">{narration}</p>}
      {player.failed && <span className="video-error" role="status">Preview unavailable</span>}
      {chrome.fullscreenError && <span className="fullscreen-error" role="status" aria-live="polite">{chrome.fullscreenError}</span>}
    </div>
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
  const select = (index: number) => player.seek(chapters[index].start);
  const onKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>, index: number) => {
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
    <div className="section-heading"><h2 id="showcase-title">See Domios at work.</h2><p>{Math.round(media.duration)} seconds inside Domios.</p></div>
    <div className="showcase-workbench">
      <div className="clip-panel" id="trailer-player"><VideoPlayer clip={media} player={player} /></div>
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
    </div>
  </section>;
}
