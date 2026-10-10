import React from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { Terminal } from "@xterm/xterm";
import { transcriptRows } from "@codeterm/chat-core";
import {
  commitTranscriptBatch,
  readTranscriptWindow,
  useTranscriptWindow,
  clearTranscriptWindows,
} from "./src/components/Terminal/transcriptStore";
import { LocalArtifactBlock } from "./packages/chat-ui/src/LocalArtifactBlock";
import { LocalArtifactScope } from "./packages/chat-ui/src/LocalArtifactScope";
import { configureLocalPreviews } from "./src/lib/configureLocalPreviews";
import { __setInvokeForTest, __setListenForTest } from "./src/lib/bridge";
import { writeTerminalOutput } from "./src/hooks/terminal/terminalOutputWrite";
const keys = Array.from({ length: 8 }, (_, i) => `heap-tab-${i}:session`);
const handlers = new Map();
__setListenForTest(async (event, handler) => {
  handlers.set(event, handler);
  return () => handlers.delete(event);
});
__setInvokeForTest(async (cmd, args) => {
  if (cmd === "read_local_preview_file")
    return {
      data:
        args.kind === "image"
          ? "data:image/svg+xml;base64," +
            btoa(
              '<svg xmlns="http://www.w3.org/2000/svg"><text>' +
                args.path +
                "</text></svg>",
            )
          : "# " + args.path,
      media_type: args.kind === "image" ? "image/svg+xml" : "text/markdown",
    };
  return null;
});
configureLocalPreviews();
const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
function Chat({ tab }) {
  const win = useTranscriptWindow(tab);
  const rows = transcriptRows(win.transcript);
  return (
    <section aria-label={tab}>
      {rows.slice(-20).map((row) => (
        <p key={row.id}>{row.content.slice(0, 80)}</p>
      ))}
    </section>
  );
}
function View({ phase }) {
  return (
    <>
      <Chat tab={keys[phase % 8]} />
      {phase % 2 === 0 && (
        <LocalArtifactScope tabId={keys[0]}>
          <LocalArtifactBlock
            key={phase}
            directive={{ kind: "image", path: `/work/${phase}.svg` }}
            renderMarkdown={(text) => text}
          />
          <LocalArtifactBlock
            directive={{ kind: "document", path: `/work/${phase}.md` }}
            renderMarkdown={(text) => text}
          />
        </LocalArtifactScope>
      )}
    </>
  );
}
const terms = Array.from({ length: 2 }, () => {
  const div = document.createElement("div");
  document.body.append(div);
  const term = new Terminal({ cols: 80, rows: 12, scrollback: 200 });
  term.open(div);
  return term;
});
let nextId = 0;
window.step = async (phase) => {
  for (let batch = 0; batch < 25; batch++) {
    for (const key of keys) {
      const rows = Array.from({ length: 10 }, (_, n) => {
        const id = nextId++;
        const bytes = new Uint8Array(3072);
        crypto.getRandomValues(bytes);
        const content = btoa(String.fromCharCode(...bytes));
        return {
          id: `heap-row-${id}`,
          type: n % 3 === 0 ? "tool_result" : "assistant",
          toolId: `tool-${id}`,
          content,
          toolResult: n % 3 === 0 ? content : undefined,
          seq: id,
        };
      });
      commitTranscriptBatch(key, {
        kind: "delta",
        authority: { sourceSessionId: key, sessionBindingNonce: "heap" },
        range: null,
        rows,
      });
    }
    flushSync(() => root.render(<View phase={phase * 25 + batch} />));
    await Promise.all(
      terms.map(
        (term) =>
          new Promise((resolve) =>
            writeTerminalOutput(
              term,
              ("agent output " + nextId + "\r\n").repeat(100),
              resolve,
            ),
          ),
      ),
    );
  }
  await new Promise(requestAnimationFrame);
  return {
    rows: keys.map(
      (key) => transcriptRows(readTranscriptWindow(key).transcript).length,
    ),
    dom: document.querySelectorAll("*").length,
  };
};
window.cleanup = () => {
  root.unmount();
  terms.forEach((term) => term.dispose());
  clearTranscriptWindows();
};
window.ready = true;
