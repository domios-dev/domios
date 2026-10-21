import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { ComposerCapsuleField } from "./ComposerCapsuleField";
import { capsuleToken } from "./composerCapsules";
import type { ComposerUpload } from "./composerUploadModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test.each(["uploading", "failed"] as const)("%s capsule exposes removal and retry without hover", async (state) => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onRemove = vi.fn();
  const onRetry = vi.fn();
  const textareaRef = createRef<HTMLTextAreaElement>();
  const upload: ComposerUpload = { id: 1, name: "clip.mp4", label: "video1", blob: new Blob(["x"]), state, failure: state === "failed" ? { message: "Interrupted", retryable: true } : undefined };
  const registry = { video1: { label: "video1", kind: "video" as const, path: "", uploadId: 1 } };
  try {
    await act(async () => root.render(
      <ComposerCapsuleField tabId="opaque" value={capsuleToken("video1")} active registry={registry} textareaRef={textareaRef} marginPx={0} onRemove={onRemove} onRetry={onRetry} uploads={[upload]}>
        <textarea ref={textareaRef} aria-label="Message" defaultValue={capsuleToken("video1")} />
      </ComposerCapsuleField>,
    ));
    expect(host.querySelector(`[role="status"][aria-label="${state === "uploading" ? "Uploading" : "Failed attachment"} clip.mp4"]`)).not.toBeNull();
    const editor = host.querySelector("textarea")!;
    expect(editor.disabled).toBe(false);
    expect(editor.readOnly).toBe(false);
    const retry = host.querySelector<HTMLButtonElement>('[aria-label="Retry attachment clip.mp4"]');
    if (state === "failed") {
      expect(retry).not.toBeNull();
      await act(async () => retry!.click());
      expect(onRetry).toHaveBeenCalledExactlyOnceWith(1);
    } else expect(retry).toBeNull();
    const remove = host.querySelector<HTMLButtonElement>('[aria-label="Remove attachment clip.mp4"]');
    expect(remove).not.toBeNull();
    await act(async () => remove!.click());
    expect(onRemove).toHaveBeenCalledExactlyOnceWith({ start: 0, end: capsuleToken("video1").length, label: "video1" });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
