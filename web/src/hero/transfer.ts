import { spring, stepSpring } from './physics';
import type { Spring } from './physics';

export const messages = [
  ['Build the landing page', 'Done: 3 files, tests pass'],
  ['Review PR #42', '2 comments, approve'],
  ['Run the GPU eval', 'Score 0.91 · 4m12s'],
  ['Index the docs', '1,284 chunks saved'],
  ['Check the Linux build', 'Build passed · 38s'],
  ['Trace the slow query', 'Found a missing index'],
  ['Audit the dependencies', 'No advisories found'],
  ['Render the preview', 'Preview ready · 1080p'],
];

export type Transfer = {
  progress: Spring; shape: Spring; swell: Spring; expansion: Spring;
  landed: number; next: number; report: boolean; active: boolean;
  phase: 'depart' | 'read' | 'contract' | 'arrive'; reading: number; message: number;
};

export function makeTransfer(index: number): Transfer {
  return {
    progress: spring(0, .35), shape: spring(), swell: spring(), expansion: spring(),
    landed: -1, next: [.05, .8, .35, 1.45][index], report: false, active: false,
    phase: 'depart', reading: 0, message: [0, 2, 3, 1][index],
  };
}

export function advanceTransfer(transfer: Transfer, dt: number) {
  if (transfer.phase === 'depart' && transfer.progress.value > .43) transfer.phase = 'read';
  if (transfer.phase === 'read') {
    transfer.reading += dt;
    if (transfer.reading > 2.15) transfer.phase = 'contract';
  }
  if (transfer.phase === 'contract' && transfer.expansion.value < .025) {
    transfer.phase = 'arrive';
    transfer.progress.velocity += .35;
  }
  const reading = transfer.phase === 'read';
  const target = transfer.phase === 'arrive' ? 1 : .48 + Math.min(transfer.reading / 2.15, 1) * .055;
  stepSpring(transfer.progress, target, transfer.phase === 'arrive' ? 32 : 64, transfer.phase === 'arrive' ? 5.8 : 12, dt);
  stepSpring(transfer.expansion, reading ? 1 : 0, 105, 14, dt);
  stepSpring(transfer.shape, 0, 260, 14, dt);
  stepSpring(transfer.swell, transfer.landed < 0 ? 1 : 0, 120, 10, dt);
}
