import { useEffect, useRef, useState } from 'react';
import { defaultMedia, parseMedia } from './media';
import type { Trailer } from './media';

export function useMediaLibrary() {
  const [library, setLibrary] = useState(defaultMedia);
  useEffect(() => {
    const controller = new AbortController();
    const read = (path: string) => fetch(path, { signal: controller.signal })
      .then((response) => response.ok ? response.json() as Promise<unknown> : null).catch(() => null);
    void Promise.all([read('/media/manifest.json'), read('/media/captions.json')])
      .then(([manifest, captions]) => {
        if (!controller.signal.aborted) setLibrary(parseMedia(manifest, captions));
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
  const [soundEnabled, setSoundEnabled] = useState(false);
  const soundChoice = useRef(false);
  const playAttempt = useRef(0);
  const inViewNow = useRef(false);
  const observedInView = useRef(false);
  const policy = useRef({ loaded, pausedByUser, reducedMotion, requestedByUser });
  policy.current = { loaded, pausedByUser, reducedMotion, requestedByUser };
  const mute = () => {
    soundChoice.current = false;
    setSoundEnabled(false);
    const video = videoRef.current;
    if (video) { video.muted = true; video.loop = true; }
  };
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
      ++playAttempt.current;
      videoRef.current?.pause();
      mute();
    }
  };
  const attemptPlay = () => {
    const video = videoRef.current;
    if (!video || !canPlay()) return;
    const attempt = ++playAttempt.current;
    video.muted = !soundChoice.current;
    video.loop = !soundChoice.current;
    void video.play().then(() => {
      if (!canPlay()) video.pause();
    }).catch(() => {
      if (playAttempt.current === attempt) {
        setPlaying(false);
        setPausedByUser(true);
        policy.current.pausedByUser = true;
      }
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
    if (loaded) video.load();
  }, [loaded, clip.mp4.src, clip.webm.src]);
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (loaded && inView && visible && !pausedByUser && (!reducedMotion || requestedByUser)) {
      attemptPlay();
    } else { ++playAttempt.current; video.pause(); }
  }, [loaded, inView, visible, pausedByUser, reducedMotion, requestedByUser]);
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
      video.muted = !soundChoice.current;
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
    containerRef, videoRef, loaded, playing, failed, time, soundEnabled, seek, togglePlayback, onLoadedMetadata, onLoadedData,
    toggleSound: () => {
      if (!clip.hasNarration) return;
      if (soundChoice.current) mute();
      else {
        soundChoice.current = true;
        setSoundEnabled(true);
        const video = videoRef.current;
        if (video) { video.muted = false; video.loop = false; }
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
