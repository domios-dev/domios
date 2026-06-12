export async function pollUntil<T>(
  fn: () => T | Promise<T>,
  timeout: number,
  interval = 250,
  label = "condition",
): Promise<T> {
  const started = Date.now();
  const deadline = started + timeout;
  let delay = interval;

  for (;;) {
    const value = await fn();
    if (value) {
      return value;
    }

    const now = Date.now();
    if (now >= deadline) {
      throw new Error(`pollUntil timed out after ${timeout}ms: ${label}`);
    }

    const sleepMs = Math.min(delay, deadline - now);
    await new Promise((resolve) => setTimeout(resolve, sleepMs));
    delay = Math.min(Math.ceil(delay * 1.5), 2000);
  }
}

