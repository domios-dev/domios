export interface TerminalUndoPort {
  schedule: (callback: () => void) => void;
  reset: () => void;
}

export function createTerminalUndoBoundary(port: TerminalUndoPort) {
  let composing = false;
  let disposed = false;
  let pending: object | null = null;

  return {
    compositionStart() {
      composing = true;
      pending = null;
    },
    compositionEnd() {
      composing = false;
    },
    input(data: string) {
      pending = null;
      if (disposed || composing || data !== "\r") return;
      const submission = {};
      pending = submission;
      port.schedule(() => {
        if (disposed || composing || pending !== submission) return;
        pending = null;
        port.reset();
      });
    },
    dispose() {
      disposed = true;
      pending = null;
    },
  };
}

export function resetTerminalHelperUndo(textarea: HTMLTextAreaElement) {
  const parent = textarea.parentNode;
  if (!parent || !textarea.isConnected || textarea.value !== "") return;
  const next = textarea.nextSibling;
  const focused = textarea.ownerDocument.activeElement === textarea;
  // Removing the editable root releases its browser-owned undo steps without replacing xterm's input.
  parent.removeChild(textarea);
  parent.insertBefore(textarea, next);
  if (focused) textarea.focus({ preventScroll: true });
}
