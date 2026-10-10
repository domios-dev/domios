export type Chapter = { id: string; label: string; title: string; description: string; start: number; end: number };

export const chapters: Chapter[] = [
  { id: 'agents', label: 'Meet your agents', title: 'A desktop app. A place for your agents.', description: 'Named agents live in tabs, with a conversation and a workspace to return to.', start: 0, end: 8 },
  { id: 'board', label: 'Give work a team', title: 'Give the work a team of its own.', description: 'An Orchestrator coordinates. A Planner shapes the tasks. Named Workers move the work forward.', start: 8, end: 15 },
  { id: 'mesh', label: 'Connect your machines', title: 'Your machines. Within reach.', description: 'Pair your computers, reach their agents, and bring the reports back to your conversation.', start: 15, end: 30 },
  { id: 'plugins', label: 'Choose your tools', title: 'Your agents. Your tools.', description: 'Choose your intelligence and add capabilities through plugins. One environment for your team.', start: 30, end: 45 },
];

export type CaptionWord = { text: string; start: number; end: number };
export type Caption = { id?: number; start: number; end: number; text: string; words?: CaptionWord[] };
export type MediaAsset = { src: string; bytes?: number };
export type MusicCredit = { title: string; author: string; sourcePage: string; license: string; licenseUrl: string; credit: string };
export type Trailer = { mp4: MediaAsset; webm: MediaAsset; poster: MediaAsset; duration: number; chapters: Chapter[]; captions: Caption[]; hasNarration?: boolean; subtitles?: MediaAsset; music?: MusicCredit };

export const defaultMedia: Trailer = {
  mp4: { src: '/media/trailer.mp4' }, webm: { src: '/media/trailer.webm' },
  poster: { src: '/media/trailer-poster.webp' }, duration: 45, chapters, captions: [],
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function isAsset(value: unknown): value is MediaAsset {
  return isRecord(value) && typeof value.src === 'string' && value.src.startsWith('/media/') && !value.src.includes('..');
}

function parseMusic(value: unknown): MusicCredit | undefined {
  if (!isRecord(value) || typeof value.title !== 'string' || typeof value.author !== 'string'
    || typeof value.license !== 'string' || typeof value.credit !== 'string'
    || typeof value.sourcePage !== 'string' || !value.sourcePage.startsWith('https://')
    || typeof value.licenseUrl !== 'string' || !value.licenseUrl.startsWith('https://')) return;
  return { title: value.title, author: value.author, license: value.license, credit: value.credit,
    sourcePage: value.sourcePage, licenseUrl: value.licenseUrl };
}

export function parseCaptions(value: unknown, wordTimings?: unknown): Caption[] {
  const captions = Array.isArray(value) ? value.filter((caption): caption is Caption =>
    isRecord(caption) && typeof caption.text === 'string' && typeof caption.start === 'number'
    && typeof caption.end === 'number' && caption.start >= 0 && caption.end > caption.start) : [];
  if (!Array.isArray(wordTimings)) return captions;
  return captions.map(caption => {
    const words = wordTimings.filter(word => isRecord(word) && word.cueId === caption.id);
    if (!words.length || !words.every((word, index) => isRecord(word) && typeof word.text === 'string'
      && typeof word.start === 'number' && Number.isFinite(word.start)
      && typeof word.end === 'number' && Number.isFinite(word.end)
      && word.start >= caption.start - .001 && word.end <= caption.end + .001 && word.end > word.start
      && (!index || word.start >= words[index - 1].end))) return caption;
    const timing = words as (CaptionWord & { cueId: number })[];
    if (timing.map(word => word.text).join(' ') !== caption.text) return caption;
    return { ...caption, words: timing.map(({ text, start, end }) => ({ text, start, end })) };
  });
}

export function parseMedia(manifest: unknown, captions: unknown, wordTimings?: unknown): Trailer {
  const value = isRecord(manifest) ? manifest : {};
  const timings = Array.isArray(value.chapters) ? value.chapters : [];
  const duration = typeof value.duration === 'number' && value.duration > 0 ? value.duration : defaultMedia.duration;
  const timeline = chapters.map((chapter) => {
    const timing: unknown = timings.find((item: unknown) => isRecord(item) && item.id === chapter.id);
    return isRecord(timing) && typeof timing.start === 'number' && typeof timing.end === 'number'
      && timing.start >= 0 && timing.end > timing.start && timing.end <= duration
      ? { ...chapter, start: timing.start, end: timing.end } : chapter;
  });
  const contiguous = timeline.every((chapter, index) => chapter.start === (index ? timeline[index - 1].end : 0));
  return {
    mp4: isAsset(value.mp4) ? value.mp4 : defaultMedia.mp4,
    webm: isAsset(value.webm) ? value.webm : defaultMedia.webm,
    poster: isAsset(value.poster) ? value.poster : defaultMedia.poster,
    duration,
    chapters: contiguous && timeline[timeline.length - 1].end === duration ? timeline : chapters,
    captions: parseCaptions(captions, wordTimings),
    hasNarration: value.hasNarration === true,
    subtitles: isAsset(value.subtitles) ? value.subtitles : undefined,
    music: parseMusic(value.music),
  };
}

export function chapterAt(time: number, timeline: Chapter[] = chapters) {
  if (time >= timeline[timeline.length - 1].end) return timeline.length - 1;
  const index = timeline.findIndex((chapter) => time >= chapter.start && time < chapter.end);
  return index < 0 ? 0 : index;
}

export function timestamp(time: number) {
  return `${Math.floor(time / 60)}:${Math.floor(time % 60).toString().padStart(2, '0')}`;
}
