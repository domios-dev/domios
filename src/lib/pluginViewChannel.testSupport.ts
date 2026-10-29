/** Deterministic document-port fixture; no iframe scripts, timers or network. */
export class TestViewPort {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onmessageerror: (() => void) | null = null;
  sent: Record<string, unknown>[] = [];
  closed = false;
  postMessage(message: Record<string, unknown>) { this.sent.push(message); }
  start() {}
  close() { this.closed = true; }
  receive(data: unknown) { if (!this.closed) this.onmessage?.({ data } as MessageEvent); }
  prove() {
    const challenges = this.sent.filter(message => message.__ctChallenge != null);
    const challenge = challenges[challenges.length - 1]?.__ctChallenge;
    if (challenge != null) this.receive({ __ctProof: challenge });
  }
  get port() { return this as unknown as MessagePort; }
}
