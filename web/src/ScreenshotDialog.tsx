import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { Icon } from './Icon';
import './screenshot-dialog.css';

type Screenshot = { title: string; src: string; alt: string; description: string };

export function ScreenshotDialog({ images, selected, onSelect, onClose }: {
  images: readonly Screenshot[];
  selected: number;
  onSelect: (index: number) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const swipe = useRef<{ x: number; y: number } | null>(null);
  const image = images[selected];
  const move = (direction: number) => onSelect((selected + direction + images.length) % images.length);
  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    const previousFocus = document.activeElement;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    element.showModal();
    element.querySelector<HTMLButtonElement>('[data-gallery-close]')?.focus();
    return () => {
      element.close();
      document.body.style.overflow = overflow;
      if (previousFocus instanceof HTMLElement) previousFocus.focus({ preventScroll: true });
    };
  }, []);
  return createPortal(<dialog ref={dialog} className="screenshot-dialog" aria-labelledby="screenshot-title" aria-describedby="screenshot-description"
    onCancel={event => { event.preventDefault(); onClose(); }}
    onClick={event => { if (event.target === event.currentTarget) onClose(); }}
    onKeyDown={event => {
      if (event.code === 'ArrowLeft' || event.code === 'ArrowRight') { event.preventDefault(); move(event.code === 'ArrowRight' ? 1 : -1); }
    }}>
    <div className="screenshot-content">
      <div className="screenshot-toolbar">
        <div className="screenshot-heading"><h2 id="screenshot-title">{image.title}</h2><span aria-live="polite" aria-atomic="true">{selected + 1} / {images.length}</span></div>
        <div className="screenshot-controls">
          <button type="button" className="screenshot-previous" aria-label="Previous screenshot" onClick={() => move(-1)}><Icon name="arrow" /></button>
          <button type="button" aria-label="Next screenshot" onClick={() => move(1)}><Icon name="arrow" /></button>
          <button type="button" data-gallery-close aria-label="Close gallery" onClick={onClose}><Icon name="close" /></button>
        </div>
      </div>
      <img className="screenshot-full" src={image.src} alt={image.alt} draggable={false}
        onPointerDown={event => { if (event.pointerType === 'touch') swipe.current = { x: event.clientX, y: event.clientY }; }}
        onPointerCancel={() => { swipe.current = null; }}
        onPointerUp={event => {
          const start = swipe.current;
          swipe.current = null;
          if (!start) return;
          const dx = event.clientX - start.x;
          if (Math.abs(dx) >= 48 && Math.abs(dx) > Math.abs(event.clientY - start.y)) move(dx < 0 ? 1 : -1);
        }} />
      <p id="screenshot-description">{image.description}</p>
    </div>
  </dialog>, document.body);
}
