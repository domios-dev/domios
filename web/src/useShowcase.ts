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
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const observer = new IntersectionObserver(([entry]) => {
      setInView(entry.isIntersecting);
      if (entry.isIntersecting && !reducedMotion) setLoaded(true);
    }, { threshold: .2 });
    observer.observe(container);
    return () => observer.disconnect();
  }, [reducedMotion]);
  useEffect(() => {
    const update = () => setVisible(!document.hidden);
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (loaded && inView && visible && !pausedByUser && (!reducedMotion || requestedByUser)) {
      void video.play().catch(() => setPlaying(false));
    } else video.pause();
  }, [loaded, inView, visible, pausedByUser, reducedMotion, requestedByUser]);
  const play = () => {
    setLoaded(true);
    setPausedByUser(false);
    setRequestedByUser(true);
    const video = videoRef.current;
    if (video?.currentSrc) void video.play().catch(() => setPlaying(false));
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
      setPausedByUser(true);
      videoRef.current?.pause();
    } else play();
  };
  const onLoadedMetadata = () => {
    if (pendingSeek.current !== null && videoRef.current) {
      videoRef.current.currentTime = pendingSeek.current;
      pendingSeek.current = null;
    }
  };
  const onLoadedData = () => {
    if (inView && visible && !pausedByUser && (!reducedMotion || requestedByUser)) {
      void videoRef.current?.play().catch(() => setPlaying(false));
    }
  };
  return {
    containerRef, videoRef, loaded, playing, failed, time, seek, togglePlayback, onLoadedMetadata, onLoadedData,
    onError: () => setFailed(true),
    onPlay: () => setPlaying(true),
    onPause: () => setPlaying(false),
    onTimeUpdate: () => { if (pendingSeek.current === null) setTime(videoRef.current?.currentTime ?? 0); },
    caption: clip.captions.find((caption) => time >= caption.start && time < caption.end),
  };
}
