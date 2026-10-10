import { useEffect, useRef, useState } from 'react';
import { defaultMedia, parseMedia } from './media';
import type { Trailer } from './media';

export function useMediaLibrary() {
  const [library, setLibrary] = useState(defaultMedia);
  useEffect(() => {
    const controller = new AbortController();
    const read = (path: string) => fetch(path, { signal: controller.signal })
      .then((response) => response.ok ? response.json() as Promise<unknown> : null).catch(() => null);
    void Promise.all([read('/media/manifest.json'), read('/media/captions.json'), read('/captions/words.json')])
      .then(([manifest, captions, words]) => {
        if (!controller.signal.aborted) setLibrary(parseMedia(manifest, captions, words));
      });
    return () => controller.abort();
  }, []);
  return library;
}

export function useVideo(clip: Trailer, reducedMotion: boolean) {
  const containerRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const pendingSeek = useRef<number | null>(null);
  const [inView, setInView] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [visible, setVisible] = useState(!document.hidden);
  const [pausedByUser, setPausedByUser] = useState(false);
  const [requestedByUser, setRequestedByUser] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [failed, setFailed] = useState(false);
  const [time, setTime] = useState(0);
  const [soundLevel, setSoundLevel] = useState(0);
  const [loopEnabled, setLoopEnabled] = useState(true);
  const soundEnabled = soundLevel > 0;
  const soundChoice = useRef(0);
  const explicitSound = useRef(false);
  const mutedByUser = useRef(false);
  const ambientAttempted = useRef(false);
  const playAttempt = useRef(0);
  const inViewNow = useRef(false);
  const observedInView = useRef(false);
  const policy = useRef({ loaded, pausedByUser, reducedMotion, requestedByUser });
  policy.current = { loaded, pausedByUser, reducedMotion, requestedByUser };
  const applySound = (level: number, explicit = false) => {
    soundChoice.current = level;
    explicitSound.current = level > 0 && explicit;
    setSoundLevel(level);
    setLoopEnabled(!explicitSound.current);
    const video = videoRef.current;
    if (video) { video.volume = level || .1; video.muted = level === 0; video.loop = !explicitSound.current; }
  };
  const mute = () => applySound(0);
  const canPlay = () => inViewNow.current && !document.hidden && !policy.current.pausedByUser
    && (!policy.current.reducedMotion || policy.current.requestedByUser);
  const syncViewportVisibility = (observed = observedInView.current) => {
    const container = containerRef.current;
    const stage = container?.querySelector('.video-stage');
    const shown = observed || Boolean(stage && document.fullscreenElement === stage);
    inViewNow.current = shown;
    setInView(shown);
    if (shown && !reducedMotion) setLoaded(true);
    if (!shown) {
      ambientAttempted.current = false;
      ++playAttempt.current;
      videoRef.current?.pause();
      mute();
    }
  };
  const attemptPlay = () => {
    const video = videoRef.current;
    if (!video || !canPlay()) return;
    if (clip.hasNarration && !mutedByUser.current && !ambientAttempted.current && soundChoice.current === 0) {
      ambientAttempted.current = true;
      applySound(.1);
    }
    const attempt = ++playAttempt.current;
    const ambient = soundChoice.current === .1 && !explicitSound.current;
    video.muted = soundChoice.current === 0;
    video.loop = !explicitSound.current;
    const rejected = () => {
      if (playAttempt.current !== attempt) return;
      setPlaying(false);
      setPausedByUser(true);
      policy.current.pausedByUser = true;
    };
    void video.play().then(() => {
      if (!canPlay()) video.pause();
    }).catch(() => {
      if (playAttempt.current !== attempt) return;
      if (ambient && canPlay()) {
        mute();
        void video.play().then(() => { if (!canPlay()) video.pause(); }).catch(rejected);
      } else rejected();
    });
  };
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const observer = new IntersectionObserver(([entry]) => {
      const shown = entry.isIntersecting && entry.intersectionRatio >= .2;
      observedInView.current = shown;
      syncViewportVisibility(shown);
    }, { threshold: .2 });
    const onFullscreenChange = () => syncViewportVisibility();
    observer.observe(container);
    document.addEventListener('fullscreenchange', onFullscreenChange);
    return () => {
      observer.disconnect();
      document.removeEventListener('fullscreenchange', onFullscreenChange);
    };
  }, [reducedMotion]);
  useEffect(() => {
    const update = () => {
      setVisible(!document.hidden);
      if (document.hidden) {
        ambientAttempted.current = false;
        ++playAttempt.current;
        videoRef.current?.pause();
        mute();
      }
    };
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (loaded && inView && visible && !pausedByUser && (!reducedMotion || requestedByUser)) {
      attemptPlay();
    } else { ++playAttempt.current; video.pause(); }
  }, [loaded, inView, visible, pausedByUser, reducedMotion, requestedByUser, clip.hasNarration]);
  useEffect(() => {
    if (reducedMotion) {
      setRequestedByUser(false);
      policy.current.requestedByUser = false;
      ++playAttempt.current;
      videoRef.current?.pause();
      mute();
    }
  }, [reducedMotion]);
  useEffect(() => {
    const video = videoRef.current;
    if (!video || !playing || !('requestVideoFrameCallback' in video)) return;
    let frame = 0;
    const update = () => {
      if (pendingSeek.current === null) setTime(video.currentTime);
      frame = video.requestVideoFrameCallback(update);
    };
    frame = video.requestVideoFrameCallback(update);
    return () => video.cancelVideoFrameCallback(frame);
  }, [playing]);
  useEffect(() => {
    const video = videoRef.current;
    return () => { ++playAttempt.current; video?.pause(); };
  }, []);
  const play = () => {
    setLoaded(true);
    setPausedByUser(false);
    setRequestedByUser(true);
    policy.current.pausedByUser = false;
    policy.current.requestedByUser = true;
    const video = videoRef.current;
    if (video?.ended) { video.currentTime = 0; setTime(0); }
    if (video?.currentSrc) attemptPlay();
  };
  const seek = (next: number) => {
    const video = videoRef.current;
    pendingSeek.current = next;
    setTime(next);
    if (video && video.readyState >= 1) {
      video.currentTime = next;
      pendingSeek.current = null;
    }
    containerRef.current?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    play();
  };
  const togglePlayback = () => {
    if (playing) {
      ++playAttempt.current;
      setPausedByUser(true);
      policy.current.pausedByUser = true;
      videoRef.current?.pause();
    } else play();
  };
  const onLoadedMetadata = () => {
    const video = videoRef.current;
    if (video) {
      video.muted = soundChoice.current === 0;
      for (const track of Array.from(video.textTracks)) track.mode = 'hidden';
    }
    if (pendingSeek.current !== null && videoRef.current) {
      videoRef.current.currentTime = pendingSeek.current;
      pendingSeek.current = null;
    }
  };
  const onLoadedData = () => {
    if (inView && visible && !pausedByUser && (!reducedMotion || requestedByUser)) {
      attemptPlay();
    }
  };
  return {
    containerRef, videoRef, loaded, playing, failed, time, soundEnabled, soundAtFullVolume: soundLevel === 1,
    volume: soundLevel, loopEnabled, seek, togglePlayback, onLoadedMetadata, onLoadedData,
    setVolume: (percentage: number) => {
      const level = Math.max(0, Math.min(1, percentage / 100));
      mutedByUser.current = level === 0;
      ambientAttempted.current = true;
      applySound(level, true);
    },
    toggleSound: () => {
      if (!clip.hasNarration) return;
      if (soundChoice.current === 1) { mutedByUser.current = true; mute(); }
      else {
        mutedByUser.current = false;
        ambientAttempted.current = true;
        applySound(1, true);
        play();
      }
    },
    onEnded: () => { setPlaying(false); setPausedByUser(true); policy.current.pausedByUser = true; },
    onError: () => setFailed(true),
    onPlay: () => { if (canPlay()) setPlaying(true); else videoRef.current?.pause(); },
    onPause: () => setPlaying(false),
    onTimeUpdate: () => { if (pendingSeek.current === null) setTime(videoRef.current?.currentTime ?? 0); },
    caption: clip.captions.find((caption) => time >= caption.start && time < caption.end),
  };
}
