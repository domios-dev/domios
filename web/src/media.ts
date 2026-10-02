export const chapters = [
  { id: 'mesh', label: 'The mesh', title: 'Your machines. Within reach.', description: 'Pair your machines into a trusted mesh. Reach another workspace, send a command, and bring the result back.', tag: '01 / CONNECT' },
  { id: 'board', label: 'Plans & teams', title: 'Give the work a team of its own.', description: 'An Orchestrator coordinates and verifies. A Planner designs the tasks and gives each named Worker a personality fitted to the work.', tag: '02 / ORCHESTRATE' },
  { id: 'mesh-talk', label: 'Across machines', title: 'Some colleagues live in the other computer.', description: 'Agents reach their peers across your mesh. Send work to a machine you choose and receive the report where you started.', tag: '03 / COLLABORATE' },
  { id: 'plugins', label: 'Plugins & providers', title: 'Make room for your tools.', description: 'Choose your agents. Add providers, secret backends, and integrations through a host that keeps their permissions explicit.', tag: '04 / EXTEND' },
] as const;

export type ClipId = typeof chapters[number]['id'];
export type Caption = { start: number; end: number; text: string };
export type MediaAsset = { src: string; bytes?: number };
export type Clip = { id: ClipId; mp4: MediaAsset; webm: MediaAsset; poster: MediaAsset; captions: Caption[] };
export type MediaLibrary = Record<ClipId, Clip>;

function defaultClip(id: ClipId): Clip {
  return { id, mp4: { src: `/media/${id}.mp4` }, webm: { src: `/media/${id}.webm` },
    poster: { src: `/media/${id}-poster.webp` }, captions: [] };
}

export const defaultMedia: MediaLibrary = {
  board: defaultClip('board'), mesh: defaultClip('mesh'),
  plugins: defaultClip('plugins'), 'mesh-talk': defaultClip('mesh-talk'),
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function isAsset(value: unknown): value is MediaAsset {
  return isRecord(value) && typeof value.src === 'string' && value.src.startsWith('/media/') && !value.src.includes('..');
}

export function parseCaptions(value: unknown): Caption[] {
  return Array.isArray(value) ? value.filter((caption): caption is Caption =>
    isRecord(caption) && typeof caption.text === 'string' && typeof caption.start === 'number'
    && typeof caption.end === 'number' && caption.start >= 0 && caption.end > caption.start) : [];
}

export function parseMedia(manifest: unknown, captions: unknown): MediaLibrary {
  const library = { ...defaultMedia };
  const clips = isRecord(manifest) && Array.isArray(manifest.clips) ? manifest.clips : [];
  for (const { id } of chapters) {
    const clip = clips.find((item: unknown) => isRecord(item) && item.id === id);
    const overrides = isRecord(clip) ? clip : {};
    library[id] = {
      id,
      mp4: isAsset(overrides.mp4) ? overrides.mp4 : defaultMedia[id].mp4,
      webm: isAsset(overrides.webm) ? overrides.webm : defaultMedia[id].webm,
      poster: isAsset(overrides.poster) ? overrides.poster : defaultMedia[id].poster,
      captions: parseCaptions(isRecord(captions) ? captions[id] ?? overrides.captions : overrides.captions),
    };
  }
  return library;
}
