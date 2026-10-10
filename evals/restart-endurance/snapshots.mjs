export async function snapshotAfterEvent(events, read) {
  for (;;) {
    try { return await read(); }
    catch (error) {
      if (error.status !== 503) throw error;
      await events.next();
    }
  }
}
