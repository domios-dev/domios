import { applyDsCanvasEdit, invertDsCanvasEdit, type DsCanvasEdit } from "@codeterm/chat-core";
import type { CanvasWriteResult } from "./canvasFilePort";

export interface CanvasSessionPort {
  /** Replaces the file when it still holds `base`, or returns the text it holds now. */
  write(base: string, next: string): Promise<CanvasWriteResult>;
  /** Asks the host to read the file again. */
  reload(): void;
}

export interface CanvasSessionState {
  /** The board as shown: the file on disk with the owner's unsaved edits on top. */
  text: string | null;
  failure: string | null;
  canUndo: boolean;
  canRedo: boolean;
}

interface Entry { forward: DsCanvasEdit; backward: DsCanvasEdit }
const CONFLICT_ATTEMPTS = 3;
const applyAll = (text: string, edits: readonly DsCanvasEdit[]) => edits.reduce((current, edit) => applyDsCanvasEdit(current, edit) ?? current, text);

/**
 * One open canvas file. Owner edits apply at once and save in order, one write at a time; a file
 * that changed underneath is read back and the edits land on top of it, so an agent's change is
 * never dropped. Unsaved edits stay in memory until a write succeeds.
 */
export class CanvasFileSession {
  private disk: string | null = null;
  private pending: DsCanvasEdit[] = [];
  private undoStack: Entry[] = [];
  private redoStack: Entry[] = [];
  private writing = false;
  private staleRead = false;
  private failure: string | null = null;
  private holds = 0;
  private held: string | null = null;
  private listeners = new Set<() => void>();
  private state: CanvasSessionState = { text: null, failure: null, canUndo: false, canRedo: false };

  private readonly port: CanvasSessionPort;

  constructor(port: CanvasSessionPort) {
    this.port = port;
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  snapshot = () => this.state;

  /** A read of the file. Reads that race a write are dropped and repeated once it lands. */
  observe(text: string): void {
    if (this.writing) { this.staleRead = true; return; }
    if (this.holds > 0) { this.held = text; return; }
    if (text === this.disk) return;
    this.disk = text;
    this.publish();
  }

  /** Holds file updates during a gesture or a note edit; the release applies the latest one. */
  hold(): () => void {
    this.holds++;
    let released = false;
    return () => {
      if (released) return;
      released = true;
      this.holds--;
      if (this.holds > 0 || this.held === null) return;
      const text = this.held;
      this.held = null;
      this.observe(text);
    };
  }

  /** Applies an owner edit; `false` when it does not apply, such as deleting the last item. */
  commit(edit: DsCanvasEdit): boolean {
    return this.apply(edit, true);
  }

  undo(): boolean {
    return this.step(this.undoStack, this.redoStack, "backward");
  }

  redo(): boolean {
    return this.step(this.redoStack, this.undoStack, "forward");
  }

  retry(): void {
    this.failure = null;
    this.publish();
    void this.flush();
  }

  private step(from: Entry[], to: Entry[], direction: keyof Entry): boolean {
    const entry = from.pop();
    if (!entry) return false;
    if (this.apply(entry[direction], false)) to.push(entry);
    else this.publish();
    return true;
  }

  private apply(edit: DsCanvasEdit, record: boolean): boolean {
    const base = this.state.text;
    if (base === null) return false;
    const next = applyDsCanvasEdit(base, edit);
    if (next === null) return false;
    if (record) {
      const backward = invertDsCanvasEdit(base, edit);
      if (backward) this.undoStack.push({ forward: edit, backward });
      this.redoStack = [];
    }
    this.pending.push(edit);
    this.publish();
    void this.flush();
    return true;
  }

  private async flush(): Promise<void> {
    if (this.writing || this.failure !== null || this.pending.length === 0 || this.disk === null) return;
    this.writing = true;
    const edits = this.pending.slice();
    let base = this.disk;
    try {
      for (let attempt = 1; ; attempt++) {
        const next = applyAll(base, edits);
        if (next === base) break;
        const result = await this.port.write(base, next);
        if (result.status === "written") {
          base = next;
          // A read held during a gesture predates this write; read the file again instead.
          if (this.held !== null) { this.held = null; this.staleRead = true; }
          break;
        }
        base = result.current;
        if (attempt >= CONFLICT_ATTEMPTS) throw new Error("The canvas kept changing on disk while it was being saved.");
      }
      this.pending.splice(0, edits.length);
    } catch (error) {
      this.failure = error instanceof Error ? error.message : String(error);
    } finally {
      this.disk = base;
      this.writing = false;
    }
    this.publish();
    if (this.staleRead) {
      this.staleRead = false;
      this.port.reload();
    }
    void this.flush();
  }

  private publish(): void {
    const text = this.disk === null ? null : applyAll(this.disk, this.pending);
    this.state = { text, failure: this.failure, canUndo: this.undoStack.length > 0, canRedo: this.redoStack.length > 0 };
    for (const listener of this.listeners) listener();
  }
}
