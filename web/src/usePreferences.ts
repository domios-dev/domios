import { useEffect, useState } from 'react';

export function useMediaQuery(query: string) {
  const [matches, setMatches] = useState(() => matchMedia(query).matches);
  useEffect(() => {
    const media = matchMedia(query);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, [query]);
  return matches;
}

export function useTheme() {
  const systemLight = useMediaQuery('(prefers-color-scheme: light)');
  const [choice, setChoice] = useState<string | null>(() => {
    try { return localStorage.getItem('domios-theme'); } catch { return null; }
  });
  const theme = choice === 'light' || choice === 'dark' ? choice : systemLight ? 'light' : 'dark';
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.querySelectorAll('meta[name="theme-color"]').forEach(meta => meta.setAttribute('content', theme === 'dark' ? '#0d1117' : '#f6f8fa'));
  }, [theme]);
  const toggle = () => {
    const next = theme === 'dark' ? 'light' : 'dark';
    setChoice(next);
    try { localStorage.setItem('domios-theme', next); } catch { /* Private browsing may deny storage. */ }
  };
  return { theme, toggle };
}
