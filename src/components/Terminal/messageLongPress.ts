export interface TouchPoint {
  pointerId: number;
  x: number;
  y: number;
}

export function createMessageLongPress(deps: {
  schedule: (callback: () => void, delay: number) => unknown;
  cancel: (handle: unknown) => void;
  open: () => void;
  owned: (owned: boolean) => void;
}) {
  let point: TouchPoint | null = null;
  let timer: unknown;
  let fired = false;
  const cancel = () => {
    if (timer !== undefined) deps.cancel(timer);
    timer = undefined;
    point = null;
    fired = false;
    deps.owned(false);
  };
  return {
    start(next: TouchPoint) {
      cancel();
      point = next;
      deps.owned(true);
      timer = deps.schedule(() => {
        timer = undefined;
        if (!point) return;
        fired = true;
        deps.open();
      }, 450);
    },
    move(next: TouchPoint) {
      if (point?.pointerId === next.pointerId && Math.hypot(next.x - point.x, next.y - point.y) > 8) cancel();
    },
    end(pointerId: number) {
      if (point?.pointerId !== pointerId) return false;
      const consumed = fired;
      cancel();
      return consumed;
    },
    cancel,
  };
}
