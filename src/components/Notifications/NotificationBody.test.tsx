// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { configureLocalArtifactLoader, LocalArtifactScope } from "@codeterm/chat-ui";
import { InlineAttachmentText } from "../../../packages/chat-ui/src/AttachmentCapsule";
import { NotificationBody } from "./NotificationBody";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("notification and inline capsule bodies load images with their owning tab", async () => {
  const loader = vi.fn(async () => ({ data: "data:image/png;base64,aGVsbG8=", media_type: "image/png" }));
  configureLocalArtifactLoader(loader);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<>
      <NotificationBody text="@ds-image /work/notice.png" tabId="mesh:peer/owner" />
      <LocalArtifactScope tabId="local-owner"><InlineAttachmentText segments={[
        { attachment: { kind: "file", path: "/work/note.txt", label: "note" } },
        { text: "\n@ct-image /work/capsule.png\n" },
      ]} /></LocalArtifactScope>
    </>));
    expect([...host.querySelectorAll("img")].map((image) => image.getAttribute("src"))).toEqual([
      "data:image/png;base64,aGVsbG8=", "data:image/png;base64,aGVsbG8=",
    ]);
    expect(loader).toHaveBeenCalledWith("/work/notice.png", "image", "mesh:peer/owner");
    expect(loader).toHaveBeenCalledWith("/work/capsule.png", "image", "local-owner");
    expect(host.querySelector('[data-attachment-path="/work/note.txt"]')).not.toBeNull();
    loader.mockClear();
    await act(async () => root.render(<NotificationBody text="@ds-image /work/unknown.png" tabId={null} />));
    expect(loader).not.toHaveBeenCalled();
    expect(host.textContent).toContain("@ds-image /work/unknown.png");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    configureLocalArtifactLoader(null);
  }
});

test("failure notifications keep opaque bodies inside a collapsed copyable disclosure", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const raw = "<opaque-error> backend diagnostics";
  try {
    await act(async () => root.render(<NotificationBody text={raw} tabId={null} failure />));
    const details = host.querySelector("details")!;
    expect(details.open).toBe(false);
    expect(details.querySelector("pre")?.textContent).toBe(raw);
    expect(details.querySelector("button")).not.toBeNull();
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    await act(async () => root.render(<NotificationBody text={raw} tabId={null} failure preview />));
    expect(host.textContent).not.toContain(raw);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
