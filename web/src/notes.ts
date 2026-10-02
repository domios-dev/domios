export type BuilderNote = { date: string; text: string; tag?: string; featured?: boolean };

export const notes: BuilderNote[] = [
  {
    date: '2026-10-02',
    text: 'Everyone was hyping JEV and I was sure it was nonsense. Then I wired it into Domios. It sits on the memory layer like it was made for it: recall got noticeably sharper.',
    tag: 'memory',
    featured: true,
  },
  {
    date: '2026-09-30',
    text: 'Shipped 1.11.11: long messages to Claude land as your own request now, and no message ever sticks in the composer again. Tested live on Claude, Codex and OpenCode.',
    tag: 'release',
  },
  {
    date: '2026-09-30',
    text: 'GPT-6.1-Sol is now the default Codex model in Domios, the day it came out.',
    tag: 'models',
  },
  {
    date: '2026-10-01',
    text: 'The web UI now follows you across machines: switch to another box in the mesh and its plans load right there.',
    tag: 'mesh',
  },
];
