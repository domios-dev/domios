import { useEffect, useRef, useState } from 'react';

export function useNoteEntrance(staticMotion: boolean) {
  const ref = useRef<HTMLElement>(null);
  const [entered, setEntered] = useState(false);
  useEffect(() => {
    const element = ref.current;
    if (!element || staticMotion || entered) return;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting) {
        setEntered(true);
        observer.disconnect();
      }
    }, { threshold: .18 });
    observer.observe(element);
    return () => observer.disconnect();
  }, [staticMotion, entered]);
  return { ref, entered };
}
