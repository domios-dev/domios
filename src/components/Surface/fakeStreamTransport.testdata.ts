import type { StreamViewport } from "./streamModel";
import type { SurfaceState } from "./surfaceChromeModel";
import type { StreamFrame, StreamHandlers, SurfaceNotice, SurfaceStreamTransport } from "./surfaceStreamTransport";

export interface FakeCall {
  method: string;
  args: Record<string, unknown>;
}

type Responder = (args: Record<string, unknown>) => unknown;

export interface FakeStreamTransport {
  transport: SurfaceStreamTransport;
  connections: StreamViewport[];
  calls: FakeCall[];
  disconnects(): number;
  connected(): boolean;
  callsTo(method: string): FakeCall[];
  respond(method: string, responder: Responder): void;
  emitOpen(): void;
  emitFrame(frame: StreamFrame): void;
  emitState(state: SurfaceState): void;
  emitNotice(notice: SurfaceNotice): void;
}

export function createFakeStreamTransport(): FakeStreamTransport {
  const connections: StreamViewport[] = [];
  const calls: FakeCall[] = [];
  const responders = new Map<string, Responder>();
  let handlers: StreamHandlers | null = null;
  let disconnects = 0;

  const live = (): StreamHandlers => {
    if (!handlers) throw new Error("fake transport is not connected");
    return handlers;
  };

  const transport: SurfaceStreamTransport = {
    connect(params, next) {
      connections.push(params);
      handlers = next;
      return () => {
        if (handlers === next) handlers = null;
        disconnects += 1;
      };
    },
    async invoke<T>(method: string, args: Record<string, unknown> = {}) {
      calls.push({ method, args });
      const responder = responders.get(method);
      return (responder ? await responder(args) : undefined) as T;
    },
  };

  return {
    transport,
    connections,
    calls,
    disconnects: () => disconnects,
    connected: () => handlers !== null,
    callsTo: (method) => calls.filter((call) => call.method === method),
    respond: (method, responder) => { responders.set(method, responder); },
    emitOpen: () => live().open(),
    emitFrame: (frame) => live().frame(frame),
    emitState: (state) => live().state(state),
    emitNotice: (notice) => live().notice(notice),
  };
}
