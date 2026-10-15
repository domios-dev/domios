import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { ComposerIssuesProvider, usePublishComposerIssues } from "./ComposerIssuesContext";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import type { AttachmentUpload } from "./attachmentUploadModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const uploads: AttachmentUpload[] = [{ id: 1, name: "note.webm", blob: new Blob(["note"]), state: "failed", failure: { message: "opaque", retryable: true } }];

it.each(["Retry", "Remove"])("opens the warning indicator and wires %s once", async action => {
  const retry = vi.fn(); const remove = vi.fn();
  function Source() {
    usePublishComposerIssues(uploads, { retry, remove });
    return null;
  }
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ComposerIssuesProvider><Source /><AgentRuntimeBanner state={null} onRetry={() => {}} /></ComposerIssuesProvider>));
    const indicator = host.querySelector<HTMLButtonElement>('button[aria-label="1 attachment failed"]')!;
    expect(indicator).not.toBeNull();
    await act(async () => indicator.click());
    expect(host.querySelector('[role="dialog"] ul[aria-label="Attachment issues"] li')).not.toBeNull();
    const control = host.querySelector<HTMLButtonElement>(`button[aria-label="${action} attachment note.webm"]`)!;
    expect(control).not.toBeNull();
    await act(async () => control.click());
    expect(action === "Retry" ? retry : remove).toHaveBeenCalledExactlyOnceWith(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
