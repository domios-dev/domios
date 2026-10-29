export type FrameIntent = { shown: boolean; frameFocused: boolean; activated: boolean; generation: number };
type Receiver = (message: Record<string, unknown>, intent: FrameIntent) => void;
type Packet = { message: Record<string, unknown>; intent: FrameIntent };
const registrations = new Map<string, PluginViewChannel>();
const MAX_QUEUED = 32;
const MAX_BYTES = 256 * 1024;
let installed = false;

// Structured-clone objects must fit a bounded JSON envelope before either dispatch path.
function envelopeBytes(value: unknown): number | null {
  let bytes = 0, nodes = 0;
  const seen = new Set<object>();
  const visit = (item: unknown, depth: number): boolean => {
    if (++nodes > 8192 || depth > 16) return false;
    if (item === null || item === undefined || typeof item === "boolean") bytes += 5;
    else if (typeof item === "number") { if (!Number.isFinite(item)) return false; bytes += 32; }
    else if (typeof item === "string") {
      if (item.length > MAX_BYTES) return false;
      bytes += new TextEncoder().encode(JSON.stringify(item)).byteLength;
    } else if (typeof item === "object") {
      if (seen.has(item) || (!Array.isArray(item) && Object.getPrototypeOf(item) !== Object.prototype && Object.getPrototypeOf(item) !== null)) return false;
      seen.add(item);
      bytes += 2;
      if (Array.isArray(item)) {
        if (item.length > 8192) return false;
        for (const child of item) {
          bytes += 1;
          if (!visit(child, depth + 1)) return false;
        }
      } else for (const key in item) {
        if (!Object.prototype.hasOwnProperty.call(item, key)) continue;
        const child = (item as Record<string, unknown>)[key];
        if (key.length > MAX_BYTES) return false;
        bytes += new TextEncoder().encode(JSON.stringify(key)).byteLength + 2;
        if (bytes > MAX_BYTES || !visit(child, depth + 1)) return false;
      }
    } else return false;
    return bytes <= MAX_BYTES;
  };
  return visit(value, 0) ? bytes : null;
}

type DocumentLifecycle = {retire(): Promise<void>; register(): Promise<void>};

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
  private challenged = 0;
  private proven = false;
  private registering = 0;
  private dead = false;
  private incoming: Packet[] = [];
  private outgoing: Record<string, unknown>[] = [];
  private queuedBytes = 0;
  private receiver: Receiver | null = null;
  private ready: (() => void) | null = null;
  private invalid: (() => void) | null = null;
  private intent = (): FrameIntent => ({ shown: false, frameFocused: false, activated: false, generation: -1 });

  constructor(readonly id: string, private lifecycle?: DocumentLifecycle) {
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
      if (!message || typeof message !== "object" || Array.isArray(message) || envelopeBytes(message) === null) { this.fail(); return; }
      if (message.__ctProof != null) {
        if (message.__ctProof !== this.challenge || this.challenged !== this.challenge || this.proven || this.registering === this.challenge) return;
        const challenge = this.challenge;
        this.registering = challenge;
        this.transition(challenge, this.lifecycle?.register, () => {
          this.proven = true;
          this.flush();
          this.ready?.();
        });
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
    this.challenged = 0;
    this.challenge += 1;
    const challenge = this.challenge;
    this.transition(challenge, this.lifecycle?.retire, () => {
      if (this.port) {
        this.challenged = challenge;
        this.port.postMessage({ __ctChallenge: challenge });
      }
    });
  }

  private transition(challenge: number, work: (() => Promise<void>) | undefined, done: () => void) {
    const current = () => !this.dead && this.challenge === challenge;
    try {
      if (!work) { if (current()) done(); return; }
      void work().then(() => { if (current()) done(); }, () => { if (current()) this.fail(); });
    } catch { if (current()) this.fail(); }
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
