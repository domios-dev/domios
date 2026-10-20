import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { ComposerIssuesProvider, usePublishComposerIssues } from "./ComposerIssuesContext";
import { AttachmentUploadNotice } from "./AttachmentUploadNotice";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import type { AttachmentUpload } from "./attachmentUploadModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const uploads: AttachmentUpload[] = [{ id: 1, name: "note.webm", blob: new Blob(["note"]), state: "failed", failure: { message: "opaque", retryable: true } }];

it.each(["Retry", "Remove"])("quiet upload feedback wires %s once without a popup", async action => {
  const retry = vi.fn(); const remove = vi.fn();
  function Source() {
    usePublishComposerIssues(uploads, { retry, remove });
    return null;
  }
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ComposerIssuesProvider><Source /><AgentRuntimeBanner state={null} onRetry={() => {}} /><AttachmentUploadNotice upload={uploads[0]} onRetry={() => retry(1)} onRemove={() => remove(1)} /></ComposerIssuesProvider>));
    expect(host.querySelector('button[aria-label="1 attachment failed"]')).toBeNull();
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(host.querySelector('[role="note"]')).not.toBeNull();
    const control = host.querySelector<HTMLButtonElement>(`button[aria-label="${action} attachment note.webm"]`)!;
    expect(control).not.toBeNull();
    await act(async () => control.click());
    expect(action === "Retry" ? retry : remove).toHaveBeenCalledExactlyOnceWith(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
