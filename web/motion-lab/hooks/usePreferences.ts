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

export function useLabPreferences() {
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');
  const dark = useMediaQuery('(prefers-color-scheme: dark)');
  const [theme, setTheme] = useState(() => new URLSearchParams(location.search).get('theme'));
  const resolvedTheme = theme ?? (dark ? 'dark' : 'light');
  useEffect(() => { document.documentElement.dataset.theme = resolvedTheme; }, [resolvedTheme]);
  return { reducedMotion, theme: resolvedTheme, toggleTheme: () => setTheme(resolvedTheme === 'dark' ? 'light' : 'dark') };
}
