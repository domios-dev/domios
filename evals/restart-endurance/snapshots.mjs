export async function snapshotAfterEvent(events, read, acceptsEvent = () => true) {
  for (;;) {
    try { return await read(); }
    catch (error) {
      if (error.status !== 503) throw error;
      let event;
      do { event = await events.next(); } while (!acceptsEvent(event));
    }
  }
}

export async function tabSnapshot({ events, createEvents, summaries, state }) {
  const owned = !events;
  events ??= createEvents();
  try {
    if (owned) await events.open();
    return await snapshotAfterEvent(events, async () => {
      const [projection, mux] = await Promise.all([summaries(), state()]);
      const full = Object.fromEntries(Object.values(mux.tabs ?? {}).map((record) => [record.id, record]));
      return projection.map((summary) => ({ ...summary, ...full[summary.id] }));
    }, (event) => event.event_type === "mux-state-changed");
  } finally { if (owned) events.close(); }
}
