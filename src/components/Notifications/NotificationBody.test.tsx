// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { configureLocalArtifactLoader, LocalArtifactScope } from "@codeterm/chat-ui";
import { InlineAttachmentText } from "../../../packages/chat-ui/src/AttachmentCapsule";
import { NotificationBody } from "./NotificationBody";
import { createChatErrorNotice } from "../Terminal/chatErrorNotice";
import { planFileDrop } from "../Terminal/composerTransferModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("notification and inline capsule bodies load images with their owning tab", async () => {
  const loader = vi.fn(async () => ({ data: "data:image/png;base64,aGVsbG8=", media_type: "image/png" }));
  configureLocalArtifactLoader(loader);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<>
      <NotificationBody text="<ds-image src='/work/notice.png' />" tabId="mesh:peer/owner" />
      <LocalArtifactScope tabId="local-owner"><InlineAttachmentText segments={[
        { attachment: { kind: "file", path: "/work/note.txt", label: "note" } },
        { text: "\n<ds-image src='/work/capsule.png' />\n" },
      ]} /></LocalArtifactScope>
    </>));
    expect([...host.querySelectorAll("img")].map((image) => image.getAttribute("src"))).toEqual([
      "data:image/png;base64,aGVsbG8=", "data:image/png;base64,aGVsbG8=",
    ]);
    expect(loader).toHaveBeenCalledWith("/work/notice.png", "image", "mesh:peer/owner");
    expect(loader).toHaveBeenCalledWith("/work/capsule.png", "image", "local-owner");
    expect(host.querySelector('[data-attachment-path="/work/note.txt"]')).not.toBeNull();
    loader.mockClear();
    await act(async () => root.render(<NotificationBody text="<ds-image src='/work/unknown.png' />" tabId={null} />));
    expect(loader).not.toHaveBeenCalled();
    expect(host.textContent).toContain("<ds-image src='/work/unknown.png' />");
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

test("incomplete, quoted and legacy notification cards never read from the originating tab", async () => {
  const loader = vi.fn(async () => ({ data: "unused", media_type: "text/markdown" }));
  configureLocalArtifactLoader(loader);
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    for (const text of ["<ds-document src='/work/notice.md' /", "```text\n<ds-document src='/work/notice.md' />\n```", "<context>\n<ds-document src='/work/notice.md' />\n</context>", "@ds-document /work/notice.md", "@ct-document /work/notice.md"]) {
      await act(async () => root.render(<NotificationBody text={text} tabId="mesh:peer/origin" />));
    }
    expect(loader).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount());
    configureLocalArtifactLoader(null);
  }
});

test("a rename failure preview shows its plan reason instead of repeating the heading", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const reason = "plan_limit_locked: this tab is above your plan limit";
  try {
    await act(async () => root.render(<NotificationBody
      text={JSON.stringify({ code: "tab_rename_failed", message: reason })}
      tabId="locked" failure preview operation="Could not rename the tab" />));
    expect(host.textContent).toBe(reason);
  } finally {
    await act(async () => root.unmount());
  }
});


test("notification text preserves native paths and markup literally in previews and details", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const text = String.raw`C:\Users\dev\.codex\hooks.json **literal** <opaque>`;
  try {
    for (const preview of [true, false]) {
      for (const tabId of [null, "owner"]) {
        await act(async () => root.render(<NotificationBody text={text} tabId={tabId} preview={preview} />));
        expect(host.textContent).toBe(text);
        expect(host.querySelector("strong, opaque")).toBeNull();
      }
    }
  } finally {
    await act(async () => root.unmount());
  }
});


test("a chat toast shows an app-authored cause as its body while a raw transport error keeps generic text with Details", async () => {
  const upsertToast = vi.fn();
  const publish = (error: string) => {
    createChatErrorNotice("t", { upsertToast, dismissToast: () => {} }).show(error);
    return upsertToast.mock.lastCall![0] as { title: string; body: string };
  };
  const cause = "This drop contains no readable file. Use the file picker to attach the image.";
  const drop = publish(planFileDrop({ types: ["text/uri-list"] }).error!);
  const raw = "/api/tabs/t/send → HTTP 502: upstream connection reset";
  const transport = publish(raw);
  const host = document.createElement("div"); const root = createRoot(host);
  const render = async (toast: { title: string; body: string }, preview: boolean) =>
    act(async () => root.render(<NotificationBody text={toast.body} tabId="t" failure operation={toast.title} preview={preview} />));
  try {
    await render(drop, true);
    expect(host.textContent).toBe(cause);
    await render(drop, false);
    expect(host.textContent).toBe(cause);
    expect(host.querySelector("details")).toBeNull();
    await render(transport, true);
    expect(host.textContent).toBe("Something went wrong while completing this operation");
    await render(transport, false);
    expect(host.textContent).not.toContain(cause);
    expect(host.querySelector("details pre")?.textContent).toBe(raw);
  } finally { await act(async () => root.unmount()); }
});

test("capacity notices are accurate and an unknown chat error never uses its heading as an operation", async () => {
  const host = document.createElement("div"); const root = createRoot(host);
  try {
    await act(async () => root.render(<NotificationBody text="/api/tabs/t/stop → HTTP 0: Mux client capacity reached before dispatch" tabId="t" failure operation="Chat error" />));
    expect(host.textContent).toContain("The connection is busy");
    expect(host.textContent).not.toContain("handling Chat error");
    await act(async () => root.render(<NotificationBody text="opaque failure" tabId="t" failure operation="Chat error" />));
    expect(host.textContent).not.toContain("handling Chat error");
  } finally { await act(async () => root.unmount()); }
});
