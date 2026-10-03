import { useEffect, useRef, useState } from 'react';
import type { FocusEvent as ReactFocusEvent, PointerEvent as ReactPointerEvent } from 'react';

export function usePlayerChrome(playing: boolean, failed: boolean) {
  const stageRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const wasFullscreenRef = useRef(false);
  const [pointerInside, setPointerInside] = useState(false);
  const [focused, setFocused] = useState(false);
  const [touchRevealed, setTouchRevealed] = useState(false);
  const [fullscreenSupported, setFullscreenSupported] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [fullscreenError, setFullscreenError] = useState('');

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;

    const supported = typeof stage.requestFullscreen === 'function'
      && typeof document.exitFullscreen === 'function'
      && document.fullscreenEnabled === true;
    setFullscreenSupported(supported);

    const syncFullscreen = () => {
      const active = document.fullscreenElement === stage;
      setFullscreen(active);
      if (active) {
        wasFullscreenRef.current = true;
        setFullscreenError('');
      } else if (wasFullscreenRef.current) {
        wasFullscreenRef.current = false;
        const previousFocus = previousFocusRef.current;
        previousFocusRef.current = null;
        if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
      }
    };
    const showFullscreenError = () => {
      setFullscreenError(document.fullscreenElement === stage
        ? 'Fullscreen could not be exited. Press Escape to return to the page.'
        : 'Fullscreen could not be opened. The player remains available inline.');
    };

    syncFullscreen();
    document.addEventListener('fullscreenchange', syncFullscreen);
    document.addEventListener('fullscreenerror', showFullscreenError);
    return () => {
      document.removeEventListener('fullscreenchange', syncFullscreen);
      document.removeEventListener('fullscreenerror', showFullscreenError);
    };
  }, []);

  useEffect(() => {
    if (playing) setTouchRevealed(false);
  }, [playing]);

  const onPointerEnter = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.pointerType !== 'touch') setPointerInside(true);
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.pointerType !== 'touch') setPointerInside(true);
  };
  const onPointerLeave = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.pointerType !== 'touch') setPointerInside(false);
  };
  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.pointerType !== 'touch' || !(event.target instanceof Element)) return;
    if (event.target.closest('button, a, input, select, textarea, [role="button"]')) return;
    setTouchRevealed((revealed) => !revealed);
  };
  const onFocusCapture = () => setFocused(true);
  const onBlurCapture = (event: ReactFocusEvent<HTMLDivElement>) => {
    if (!stageRef.current?.contains(event.relatedTarget as Node | null)) setFocused(false);
  };

  const toggleFullscreen = () => {
    const stage = stageRef.current;
    if (!stage || !fullscreenSupported) return;

    if (document.fullscreenElement === stage) {
      try {
        void document.exitFullscreen().catch(() => {
          setFullscreenError('Fullscreen could not be exited. Press Escape to return to the page.');
        });
      } catch {
        setFullscreenError('Fullscreen could not be exited. Press Escape to return to the page.');
      }
      return;
    }

    const activeElement = document.activeElement;
    previousFocusRef.current = activeElement instanceof HTMLElement ? activeElement : null;
    setFullscreenError('');
    try {
      void stage.requestFullscreen().catch(() => {
        setFullscreenError('Fullscreen could not be opened. The player remains available inline.');
      });
    } catch {
      setFullscreenError('Fullscreen could not be opened. The player remains available inline.');
    }
  };

  return {
    stageRef,
    fullscreenSupported,
    fullscreen,
    fullscreenError,
    visible: !playing || failed || pointerInside || focused || touchRevealed,
    toggleFullscreen,
    onPointerEnter,
    onPointerMove,
    onPointerLeave,
    onPointerDown,
    onFocusCapture,
    onBlurCapture,
  };
}
