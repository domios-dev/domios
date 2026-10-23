import { uid } from "./uid";
import type { StreamEnd, TransportClient, Unsubscribe } from "./transportClient";
import {
  EMPTY_MUX_CHANNEL_SET, decodeMuxFrame, isMuxStreamPath, muxAddChannel, muxRemoveChannel, muxStreamEnd,
  muxStreamEnded, muxStreamReady, type MuxEffect, type MuxFrame, type MuxTransition,
} from "./streamMuxModel";

type Listener = Parameters<TransportClient["subscribe"]>[1];
type Options = NonNullable<Parameters<TransportClient["subscribe"]>[2]>;

const CONTROL_TIMEOUT_MS = 15_000;

interface Channel {
  id: string;
  path: string;
  listener: Listener;
  options: Options;
  session: string | null;
  request: Promise<unknown> | null;
  detach: () => void;
}

export interface StreamMux {
  subscribe: TransportClient["subscribe"];
}

/** Carries every channel over one `/api/stream-mux` connection; each channel keeps the `subscribe` contract. */
export function createStreamMux(transport: () => TransportClient): StreamMux {
  let set = EMPTY_MUX_CHANNEL_SET;
  const channels = new Map<string, Channel>();
  let session: string | null = null;
  let closeStream: Unsubscribe | null = null;

  const run = (transition: MuxTransition, removed?: Channel): void => {
    set = transition.state;
    for (const effect of transition.effects) apply(effect, removed);
  };

  function apply(effect: MuxEffect, removed?: Channel): void {
    if (effect.type === "open-stream") openStream();
    else if (effect.type === "close-stream") {
      const close = closeStream;
      closeStream = null;
      session = null;
      close?.();
    } else if (effect.type === "request-channel") requestChannel(effect.id);
    else if (removed) releaseChannel(removed);
  }

  function openStream(): void {
    const id = uid();
    session = id;
    closeStream = transport().subscribe(`/api/stream-mux?session=${id}`, (_name, data) => {
      if (session === id) return route(decodeMuxFrame(data));
    }, {
      raw: true,
      onReady: () => { if (session === id) run(muxStreamReady(set)); },
      onEnd: (end) => {
        if (session !== id) return;
        session = null;
        closeStream = null;
        const { state, ended } = muxStreamEnded(set);
        set = state;
        for (const channelId of ended) {
          const channel = channels.get(channelId);
          channels.delete(channelId);
          if (channel) finish(channel, end);
        }
      },
    });
  }

  function requestChannel(id: string): void {
    const channel = channels.get(id);
    const owner = session;
    if (!channel || !owner) return;
    channel.session = owner;
    channel.request = transport().localJson("POST", `/api/stream-mux/${owner}/channels`, { channel: id, path: channel.path }, {
      timeoutMs: CONTROL_TIMEOUT_MS,
    }).catch((error: unknown) => {
      if (channels.get(id) === channel && session === owner) fail(channel, error);
    });
  }

  function releaseChannel(channel: Channel): void {
    const owner = channel.session;
    if (!owner || owner !== session) return;
    // Waiting for the open request keeps the release from overtaking it on another socket.
    void (channel.request ?? Promise.resolve())
      .then(() => transport().localJson("DELETE", `/api/stream-mux/${owner}/channels/${channel.id}`, undefined, { timeoutMs: CONTROL_TIMEOUT_MS }))
      .catch(() => {});
  }

  function remove(channel: Channel, serverClosed: boolean): void {
    channel.detach();
    channels.delete(channel.id);
    run(muxRemoveChannel(set, channel.id, serverClosed), channel);
  }

  function finish(channel: Channel, end: StreamEnd): void {
    channel.detach();
    channel.options.onEnd?.(end);
  }

  function fail(channel: Channel, error: unknown): void {
    remove(channel, false);
    finish(channel, { reason: "error", error });
  }

  function route(frame: MuxFrame | null): void | Promise<void> {
    const channel = frame && channels.get(frame.channel);
    if (!frame || !channel) return;
    if (frame.kind === "open") {
      if (channel.options.requireEventStream && frame.contentType?.split(";")[0] !== "text/event-stream") {
        fail(channel, new Error("Daemon response is not an event stream"));
      } else channel.options.onReady?.();
      return;
    }
    if (frame.kind === "end") {
      remove(channel, true);
      finish(channel, muxStreamEnd(frame.end, channel.path));
      return;
    }
    channel.options.onActivity?.();
    if (frame.kind === "data") return deliver(channel, frame.event, frame.data);
  }

  function deliver(channel: Channel, event: string, data: unknown): void | Promise<void> {
    try {
      const result = !channel.options.raw && typeof data === "object" && data !== null
        && "event_type" in data && typeof data.event_type === "string" && "data" in data
        ? channel.listener(data.event_type, data.data)
        : channel.listener(event, data);
      if (result instanceof Promise) {
        return result.catch((error: unknown) => { if (channels.get(channel.id) === channel) fail(channel, error); });
      }
    } catch (error) {
      if (channels.get(channel.id) === channel) fail(channel, error);
    }
  }

  return {
    subscribe(path, listener, options = {}) {
      if (options.signal?.aborted) {
        queueMicrotask(() => options.onEnd?.({ reason: "abort", error: options.signal?.reason }));
        return () => {};
      }
      const channel: Channel = { id: uid(), path, listener, options, session: null, request: null, detach: () => {} };
      const onAbort = () => {
        if (channels.get(channel.id) !== channel) return;
        remove(channel, false);
        queueMicrotask(() => options.onEnd?.({ reason: "abort", error: options.signal?.reason }));
      };
      options.signal?.addEventListener("abort", onAbort, { once: true });
      channel.detach = () => options.signal?.removeEventListener("abort", onAbort);
      channels.set(channel.id, channel);
      run(muxAddChannel(set, channel.id));
      return () => { if (channels.get(channel.id) === channel) remove(channel, false); };
    },
  };
}

const muxesByOrigin = new Map<string, { client: TransportClient; mux: StreamMux }>();

/** Routes multiplexable stream paths through the origin's single shared mux connection. */
export function withStreamMux(client: TransportClient): TransportClient {
  let shared = muxesByOrigin.get(client.origin());
  if (shared) shared.client = client;
  else {
    const holder: { client: TransportClient; mux: StreamMux } = { client, mux: createStreamMux(() => holder.client) };
    muxesByOrigin.set(client.origin(), holder);
    shared = holder;
  }
  const { mux } = shared;
  return {
    ...client,
    subscribe: (path, listener, options) => isMuxStreamPath(path)
      ? mux.subscribe(path, listener, options)
      : client.subscribe(path, listener, options),
  };
}

export function __resetStreamMuxesForTest(): void {
  muxesByOrigin.clear();
}
