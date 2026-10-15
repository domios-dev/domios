import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
  TRANSCRIPT_RETENTION_LIMIT,
  transcriptRows,
} from "@codeterm/chat-core";
import {
  clearTranscriptWindows,
  commitTranscriptBatch,
  readTranscriptWindow,
  markTranscriptSnapshotReady,
  resetTranscriptWindow,
  useTranscriptWindow,
} from "./transcriptStore";

const browser = new Window({ url: "http://localhost/retention" });
Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: browser,
});
Object.defineProperty(globalThis, "document", {
  configurable: true,
  value: browser.document,
});
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", {
  configurable: true,
  value: true,
});

function Transcript({ tab }: { tab: string }) {
  const window = useTranscriptWindow(tab);
  const rows = transcriptRows(window.transcript);
  return (
    <output aria-label="Retained rows">
      {rows.length}:{rows[rows.length - 1]?.id}
    </output>
  );
}

async function main() {
  clearTranscriptWindows();
  try {
    const original = readTranscriptWindow("cached").transcript;
    markTranscriptSnapshotReady("cached", "binding");
    assert.equal(readTranscriptWindow("cached").transcript, original);
    assert.equal(readTranscriptWindow("cached").snapshotBinding, "cached|binding");
    resetTranscriptWindow("cached");
    assert.equal(readTranscriptWindow("cached").snapshotBinding, undefined);
    markTranscriptSnapshotReady("cached", "binding");
    for (let index = 0; index < 8; index++) markTranscriptSnapshotReady(`other-${index}`, "binding");
    assert.equal(readTranscriptWindow("cached").snapshotBinding, undefined);
    clearTranscriptWindows();
    for (let cycle = 0; cycle < 30; cycle++) {
      const host = document.createElement("div");
      document.body.append(host);
      const root = createRoot(host);
      const tab = `tab-${cycle % 8}`;
      try {
        await act(async () => {
          root.render(<Transcript tab={tab} />);
          for (let batch = 0; batch < 20; batch++) {
            commitTranscriptBatch(tab, {
              kind: "delta",
              authority: { sourceSessionId: tab, sessionBindingNonce: "nonce" },
              range: null,
              rows: Array.from({ length: 50 }, (_, index) => ({
                id: `${cycle}-${batch}-${index}`,
                type: "assistant",
                content: `stream ${cycle}/${batch}/${index}`,
                seq: cycle * 1_000 + batch * 50 + index,
              })),
            });
          }
        });
        assert.equal(
          host.querySelector("output")?.textContent,
          `${TRANSCRIPT_RETENTION_LIMIT}:${cycle}-19-49`,
        );
      } finally {
        await act(async () => root.unmount());
        host.remove();
      }
      for (let key = 0; key < 8; key++) {
        assert(
          transcriptRows(readTranscriptWindow(`tab-${key}`).transcript)
            .length <= TRANSCRIPT_RETENTION_LIMIT,
        );
      }
    }
  } finally {
    clearTranscriptWindows();
    browser.happyDOM.abort();
  }
  assert.equal(document.querySelectorAll("output").length, 0);
  console.log(
    "30 transcript subscription mount/unmount cycles and 30,000 streamed rows remain bounded",
  );
}

main().catch((error: unknown) => {
  console.error(error);
  process.exitCode = 1;
});
