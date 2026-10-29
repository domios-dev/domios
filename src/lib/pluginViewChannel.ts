export type FrameIntent = { shown: boolean; frameFocused: boolean; activated: boolean; generation: number };
type Receiver = (message: Record<string, unknown>, intent: FrameIntent) => void;
type Packet = { message: Record<string, unknown>; intent: FrameIntent };
const registrations = new Map<string, PluginViewChannel>();
const MAX_QUEUED = 32;
const MAX_BYTES = 256 * 1024;
let installed = false;

function install() {
  if (installed || typeof window === "undefined") return;
  installed = true;
  window.addEventListener("message", (event: MessageEvent) => {
    const id = event.data?.__ctReady;
    if (typeof id !== "string") return;
    if (event.ports.length !== 1) {
      for (const port of event.ports) port.close();
      return;
    }
    const channel = registrations.get(id);
    if (channel) channel.accept(event.source, event.ports[0]);
    else event.ports[0].close();
  });
}

/** The transferred port belongs to one document, even when its WindowProxy is reused. */
export class PluginViewChannel {
  private source: MessageEventSource | null = null;
  private offered: { source: MessageEventSource | null; port: MessagePort } | null = null;
  private port: MessagePort | null = null;
  private challenge = 0;
  private proven = false;
  private dead = false;
  private incoming: Packet[] = [];
  private outgoing: Record<string, unknown>[] = [];
  private queuedBytes = 0;
  private receiver: Receiver | null = null;
  private ready: (() => void) | null = null;
  private invalid: (() => void) | null = null;
  private intent = (): FrameIntent => ({ shown: false, frameFocused: false, activated: false, generation: -1 });

  constructor(readonly id: string) {
    if (!/^[a-f0-9]{32}$/.test(id) || registrations.has(id)) throw new Error("Invalid plugin document registration");
    install();
    registrations.set(id, this);
  }

  bind(source: Window, captureIntent: () => FrameIntent) {
    if (this.dead) return;
    if (this.source && this.source !== source) { this.fail(); return; }
    this.source = source;
    this.intent = captureIntent;
    if (this.offered) {
      const offered = this.offered;
      this.offered = null;
      this.accept(offered.source, offered.port);
    }
  }

  accept(source: MessageEventSource | null, port: MessagePort) {
    if (this.dead || (this.source && source !== this.source)) { port.close(); return; }
    if (this.port || this.offered) { port.close(); this.fail(); return; }
    if (!this.source) { this.offered = { source, port }; return; }
    this.port = port;
    port.onmessage = (event) => {
      if (this.dead) return;
      const message = event.data;
      if (!message || typeof message !== "object" || Array.isArray(message)) { this.fail(); return; }
      if (message.__ctProof != null) {
        if (message.__ctProof !== this.challenge) return;
        this.proven = true;
        this.flush();
        this.ready?.();
        return;
      }
      // Capture activation at receipt, before the proof/subscription queue or any await.
      const packet = { message, intent: this.intent() };
      if (this.proven && this.receiver) this.receiver(message, packet.intent);
      else if (this.reserve(packet)) this.incoming.push(packet);
    };
    port.onmessageerror = () => this.fail();
    port.start();
    this.observeLoad();
  }

  observeLoad() {
    if (this.dead) return;
    this.proven = false;
    this.challenge += 1;
    this.port?.postMessage({ __ctChallenge: this.challenge });
  }

  subscribe(receiver: Receiver, ready: () => void, invalid: () => void) {
    this.receiver = receiver;
    this.ready = ready;
    this.invalid = invalid;
    if (this.dead) invalid();
    else if (this.proven) { this.flush(); ready(); }
    return () => {
      if (this.receiver === receiver) { this.receiver = null; this.ready = null; this.invalid = null; }
    };
  }

  send(message: Record<string, unknown>) {
    if (this.dead) return;
    if (this.proven && this.port) this.port.postMessage(message);
    else if (this.reserve(message)) this.outgoing.push(message);
  }

  private reserve(value: unknown) {
    try {
      this.queuedBytes += new TextEncoder().encode(JSON.stringify(value)).byteLength;
      if (this.incoming.length + this.outgoing.length >= MAX_QUEUED || this.queuedBytes > MAX_BYTES) throw new Error("queue full");
      return true;
    } catch { this.fail(); return false; }
  }

  private flush() {
    if (!this.proven || this.dead) return;
    if (this.receiver) {
      const packets = this.incoming.splice(0);
      for (const packet of packets) {
        if (this.dead || !this.proven) break;
        this.receiver(packet.message, packet.intent);
      }
    }
    for (const message of this.outgoing.splice(0)) {
      if (this.dead || !this.proven) break;
      this.port?.postMessage(message);
    }
    if (!this.incoming.length && !this.outgoing.length) this.queuedBytes = 0;
  }

  private fail() {
    if (this.dead) return;
    this.dispose();
    this.invalid?.();
  }

  dispose() {
    this.dead = true;
    this.proven = false;
    this.port?.close();
    this.offered?.port.close();
    this.incoming = [];
    this.outgoing = [];
    if (registrations.get(this.id) === this) registrations.delete(this.id);
  }
}
