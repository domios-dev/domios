export function dreamRefresh(load: () => Promise<void>, active: () => boolean) {
  let running = false;
  let pending = false;
  return async () => {
    pending = true;
    if (running) return;
    running = true;
    try {
      do {
        pending = false;
        await load();
      } while (pending && active());
    } finally {
      running = false;
    }
  };
}
