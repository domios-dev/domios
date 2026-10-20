export class JourneyFailure extends Error {
  constructor(reason, evidence) {
    super(reason);
    this.reason = reason;
    this.evidence = evidence;
  }
}

const everyEvent = () => true;

// Re-reads state once per drained event burst; the deadline belongs to the Events signal.
export async function observe({ events, read, judge, filter = everyEvent, label }) {
  let last = null;
  let pending = label;
  for (;;) {
    let snapshot;
    try {
      snapshot = await read();
    } catch (error) {
      if (error?.status !== 503) throw error;
      snapshot = undefined;
    }
    if (snapshot !== undefined) {
      last = snapshot;
      const verdict = judge(snapshot);
      if (verdict?.fail) throw new JourneyFailure(verdict.fail, snapshot);
      if (verdict && "done" in verdict) return verdict.done;
      if (verdict?.pending) pending = verdict.pending;
    }
    try {
      let event;
      do { event = await events.next(); } while (!filter(event));
      if (event?.event_type === "overflow") pending = `${pending} (event stream overflow; state re-read)`;
      events.drain();
    } catch (error) {
      if (error?.message === "deadline") throw new JourneyFailure(`deadline: ${pending}`, last);
      throw error;
    }
  }
}

export function touchesTab(id) {
  return (event) => event?.event_type === "overflow" || event?.event_type === "mux-state-changed" ||
    event?.data?.tab_id === id || event?.tab_id === id ||
    (typeof event?.event_type === "string" && event.event_type.includes(id));
}

export function touchesPlan(planId) {
  return (event) => event?.event_type === "overflow" || event?.event_type === "plan-changed" ||
    event?.event_type === "agent-report" || event?.event_type === "mux-state-changed" ||
    event?.data?.plan_id === planId;
}

export function anyOf(...filters) {
  return (event) => filters.some((filter) => filter(event));
}
