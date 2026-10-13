import { planOutputResync } from "./outputResync";

export interface OutputCursor {
    boot: string;
    ptyId: string;
    offset: number;
}
export interface OutputBytesFrame {
    boot: string;
    ptyId: string;
    offset: number;
    bytes: Uint8Array;
}
export function planOutputReplay(cursor: OutputCursor | undefined, frame: OutputBytesFrame): {
    bytes: Uint8Array;
    cursor: OutputCursor;
} {
    const sameRing = cursor?.boot === frame.boot && cursor.ptyId === frame.ptyId;
    const previous = sameRing ? cursor.offset : 0;
    const start = frame.offset - frame.bytes.length;
    const skip = Math.max(0, previous - start);
    return {
        bytes: frame.bytes.slice(skip),
        cursor: { boot: frame.boot, ptyId: frame.ptyId, offset: Math.max(previous, frame.offset) },
    };
}
export class TerminalOutputReplay {
    private cursors = new WeakMap<object, Map<string, OutputCursor>>();
    resync(terminal: object, tabId: string, boot: string, ptyId: string, from: number, reportGap: () => void): void {
        let tabs = this.cursors.get(terminal);
        if (!tabs) {
            tabs = new Map();
            this.cursors.set(terminal, tabs);
        }
        const previous = tabs.get(tabId);
        const result = planOutputResync(this.offset(terminal, tabId, boot, ptyId), from);
        if (!result.reportGap) return;
        tabs.set(tabId, { boot, ptyId, offset: result.offset });
        try {
            reportGap();
        } catch (error) {
            if (previous) tabs.set(tabId, previous);
            else tabs.delete(tabId);
            throw error;
        }
    }
    offset(terminal: object, tabId: string, boot: string, ptyId: string): number {
        const cursor = this.cursors.get(terminal)?.get(tabId);
        return cursor?.boot === boot && cursor.ptyId === ptyId ? cursor.offset : 0;
    }
    deliver(terminal: object, tabId: string, frame: OutputBytesFrame, write: (bytes: Uint8Array) => void): void {
        let tabs = this.cursors.get(terminal);
        if (!tabs) {
            tabs = new Map();
            this.cursors.set(terminal, tabs);
        }
        const result = planOutputReplay(tabs.get(tabId), frame);
        if (result.bytes.length === 0)
            return;
        write(result.bytes);
        tabs.set(tabId, result.cursor);
    }
}
