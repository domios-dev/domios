export type IconName = 'close' | 'expand' | 'arrow' | 'download' | 'github' | 'sun' | 'moon' | 'terminal' | 'grid' | 'mesh' | 'layers' | 'branch' | 'memory' | 'play' | 'pause' | 'plus' | 'clock' | 'browser' | 'microphone' | 'lock';
const paths: Record<IconName, string> = {
  close: 'M6 6l12 12M18 6 6 18',
  expand: 'M3 9V3h6m6 0h6v6m0 6v6h-6m-6 0H3v-6',
  clock: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM12 7v5l3 2',
  browser: 'M3 4h18v16H3ZM3 9h18M6 6.5h.01m3 0h.01',
  microphone: 'M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0ZM6 11v1a6 6 0 0 0 12 0v-1M12 18v4m-4 0h8',
  lock: 'M5 10h14v11H5ZM8 10V6a4 4 0 0 1 8 0v4m-4 5v2',
  plus: 'M12 5v14M5 12h14',
  arrow: 'M5 12h14m-6-6 6 6-6 6',
  download: 'M12 3v12m-5-5 5 5 5-5M5 16v4h14v-4',
  github: 'M9 19c-4 1-4-2-6-2m12 5v-4c0-1 .1-2-.6-2.5 3-.3 6-1.5 6-6.5 0-1.5-.5-2.5-1.4-3.5.2-1 .2-2.5-.5-3.5-1.5 0-3 1-4 1.5a13 13 0 0 0-6 0C7.5 3 6 2 4.5 2c-.7 1-.7 2.5-.5 3.5C3 6.5 2.5 7.5 2.5 9c0 5 3 6.2 6 6.5-.7.5-.7 1.5-.7 2.5v4',
  sun: 'M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0',
  moon: 'M20 14A8 8 0 0 1 10 4a8 8 0 1 0 10 10Z',
  terminal: 'm6 8 4 4-4 4m7 0h5M3 3h18v18H3Z',
  grid: 'M3 3h7v7H3Zm11 0h7v7h-7ZM3 14h7v7H3Zm11 0h7v7h-7Z',
  mesh: 'M10 5a2 2 0 1 0 4 0 2 2 0 1 0-4 0M3 19a2 2 0 1 0 4 0 2 2 0 1 0-4 0m14 0a2 2 0 1 0 4 0 2 2 0 1 0-4 0M11 7 6 17m7-10 5 10M7 19h10',
  layers: 'm12 3 10 5-10 5L2 8Zm-10 9 10 5 10-5M2 16l10 5 10-5',
  branch: 'M6 3v12a3 3 0 1 0 0 6 3 3 0 0 0 0-6m0-7c0 6 12 0 12 6m0 0a3 3 0 1 0 0 6 3 3 0 0 0 0-6',
  memory: 'M4 5h16v14H4ZM8 9h8m-8 6h5M8 2v3m8-3v3M8 19v3m8-3v3',
  play: 'm9 5 11 7-11 7Z',
  pause: 'M8 5v14M16 5v14',
};
export function Icon({ name, size = 20 }: { name: IconName; size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>;
}
export { BrandMark } from './BrandMark';
