export function dreamRefresh(load: () => Promise<void>, active: () => boolean) {
  let running: Promise<void> | null = null;
  let pending = false;
  return () => {
    pending = true;
    if (running) return running;
    running = (async () => {
      try {
        do {
          pending = false;
          await load();
        } while (pending && active());
      } finally {
        running = null;
      }
    })();
    return running;
  };
}
