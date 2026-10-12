import type { StreamEnd, TransportClient, Unsubscribe } from "./transportClient";

type Listener = (name: string, data: unknown) => void;
type Options = NonNullable<Parameters<TransportClient["subscribe"]>[2]>;
type Connect = TransportClient["subscribe"];
interface Consumer { listener: Listener; options: Options; stop: Unsubscribe }
interface Connection { path: string; consumers: Set<Consumer>; close: Unsubscribe; generation: number }

export function eventStreamKey(path: string): string | undefined {
  const url = new URL(path, "http://daemon");
  if (url.pathname !== "/api/events/stream"
    && !(url.pathname === "/api/mesh/relay-stream" && url.searchParams.get("path")?.split("?")[0] === "/api/events/stream")) return;
  url.searchParams.delete("since");
  url.searchParams.sort();
  return url.pathname + url.search;
}


function replayCursor(data: unknown): string | undefined {
  if (typeof data !== "object" || data === null) return;
  if ("event_type" in data && data.event_type === "overflow" && "data" in data) {
    const reset = data.data;
    if (typeof reset === "object" && reset !== null && "boot" in reset && typeof reset.boot === "string"
      && "next_seq" in reset && typeof reset.next_seq === "number") return `${reset.boot}:${reset.next_seq - 1}`;
  }
  if ("boot" in data && typeof data.boot === "string" && "seq" in data && typeof data.seq === "number") {
    return `${data.boot}:${data.seq}`;
  }
}

export function createEventSubscriptionRegistry(connect: Connect) {
  const connections = new Map<string, Connection>();
  const cursors = new Map<string, string>();
  let generation = 0;
  const open = (key: string, connection: Connection) => {
    const current = connection.generation = ++generation;
    const valid = () => connections.get(key) === connection && connection.generation === current;
    connection.close = connect(connection.path, (name, data) => {
      if (!valid()) return;
      const cursor = replayCursor(data);
      if (cursor) {
        const url = new URL(connection.path, "http://daemon");
        cursors.set(key, cursor);
        url.searchParams.set("since", cursor);
        connection.path = url.pathname + url.search;
      }
      const failures: unknown[] = [];
      for (const consumer of [...connection.consumers]) {
        if (!valid()) break;
        if (!connection.consumers.has(consumer)) continue;
        try {
          if (!consumer.options.raw && typeof data === "object" && data !== null
            && "event_type" in data && typeof data.event_type === "string" && "data" in data) {
            consumer.listener(data.event_type, data.data);
          } else consumer.listener(name, data);
        } catch (error) { failures.push(error); }
      }
      if (failures.length) throw failures[0];
    }, {
      raw: true,
      onActivity: () => {
        if (valid()) for (const consumer of [...connection.consumers]) consumer.options.onActivity?.();
      },
      onEnd: (end: StreamEnd) => {
        if (!valid()) return;
        connections.delete(key);
        const consumers = [...connection.consumers];
        for (const consumer of consumers) consumer.stop();
        for (const consumer of consumers) consumer.options.onEnd?.(end);
      },
    });
  };
  return {
    subscribe(path: string, listener: Listener, options: Options = {}): Unsubscribe {
      const key = eventStreamKey(path);
      if (key === undefined) return connect(path, listener, options);
      if (options.signal?.aborted) {
        options.onEnd?.({ reason: "abort", error: options.signal.reason });
        return () => {};
      }
      let connection = connections.get(key);
      const created = !connection;
      if (!connection) {
        const cursor = cursors.get(key);
        if (cursor) {
          const url = new URL(path, "http://daemon");
          url.searchParams.set("since", cursor);
          path = url.pathname + url.search;
        }
        connection = { path, consumers: new Set(), close: () => {}, generation: 0 };
        connections.set(key, connection);
      }
      const owned = connection;
      const consumer: Consumer = { listener, options, stop: () => {
        options.signal?.removeEventListener("abort", abort);
        if (!owned.consumers.delete(consumer)) return;
        if (!owned.consumers.size && connections.get(key) === owned) {
          connections.delete(key);
          owned.close();
        }
      } };
      const abort = () => {
        consumer.stop();
        options.onEnd?.({ reason: "abort", error: options.signal?.reason });
      };
      owned.consumers.add(consumer);
      options.signal?.addEventListener("abort", abort, { once: true });
      if (created) open(key, owned);
      return consumer.stop;
    },
    replace(next: Connect) {
      for (const connection of connections.values()) { connection.generation = ++generation; connection.close(); }
      connect = next;
      for (const [key, connection] of connections) open(key, connection);
    },
  };
}

export function shareEventSubscriptions(client: TransportClient, registry: ReturnType<typeof createEventSubscriptionRegistry>): TransportClient {
  return { ...client, subscribe: registry.subscribe };
}
