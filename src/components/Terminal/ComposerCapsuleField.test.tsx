import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { ComposerCapsuleField } from "./ComposerCapsuleField";
import { ComposerIssuesProvider, usePublishComposerIssues } from "./ComposerIssuesContext";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import type { ComposerUpload } from "./composerUploadModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("a failed upload's Remove control is reachable without hover and fires once", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onRemove = vi.fn();
  const upload: ComposerUpload = { id: 1, name: "bad.mp4", label: "video1", blob: new Blob(["x"]), state: "failed", failure: { message: "Unsupported video format", retryable: false } };
  const registry = { video1: { label: "video1", kind: "video" as const, path: "", uploadId: 1 } };
  const uploads = [upload];
  function Issues() {
    usePublishComposerIssues(uploads, { retry: () => {}, remove: () => onRemove() });
    return <AgentRuntimeBanner state={null} onRetry={() => {}} />;
  }
  try {
    await act(async () => root.render(
      <ComposerIssuesProvider><Issues /><ComposerCapsuleField tabId="t" value={"\u2068\u2007\u2007\u00a0video1\u00a0\u2069"} registry={registry} active textareaRef={{ current: null }} marginPx={0} onRemove={onRemove} uploads={[upload]}>
        <textarea />
      </ComposerCapsuleField></ComposerIssuesProvider>,
    ));
    const indicator = host.querySelector<HTMLButtonElement>('button[aria-label="1 attachment failed"]')!;
    expect(indicator).not.toBeNull();
    await act(async () => indicator.click());
    const remove = host.querySelector<HTMLButtonElement>('[aria-label="Remove attachment bad.mp4"]');
    expect(remove).not.toBeNull();
    await act(async () => remove!.click());
    expect(onRemove).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
