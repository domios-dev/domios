import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { useWebFileDrop } from "./composerFiles";
import { useTauriFileDrop } from "../../hooks/terminal/useTauriFileDrop";
import { nativeBridge, type FileDropEvent } from "../../lib/nativeBridge";

const fixture = vi.hoisted(() => ({ web: true }));
vi.mock("../../lib/webApi", async original => ({ ...await original<typeof import("../../lib/webApi")>(), get IS_WEB() { return fixture.web; } }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test.each([true, false])("every composer point accepts a drop once (web=%s)", async web => {
  fixture.web = web;
  const upload = vi.fn(async () => {}), paths = vi.fn(), error = vi.fn();
  const originalDrop = nativeBridge.fileDrop, originalMaterialize = nativeBridge.materializeDroppedFiles;
  const materialize = vi.fn(async () => ["C:/uploads/hash.heic"]);
  let emit!: (event: FileDropEvent) => void;
  nativeBridge.fileDrop = { subscribe: async handler => { emit = handler; return () => {}; } };
  nativeBridge.materializeDroppedFiles = materialize;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  function Harness() {
    const ref = useRef<HTMLDivElement>(null);
    const drop = useWebFileDrop(paths, error, upload);
    const native = useTauriFileDrop({ containerRef: ref, onPaths: paths, enabled: !web, onError: error });
    return <div ref={ref} {...drop.handlers}><div data-testid="chat-composer-shell">
      <span data-point="left" /><button data-point="paperclip" onDrop={event => event.stopPropagation()}>Attach</button>
      <textarea data-point="text" /><div data-point="middle" onDragOver={event => event.stopPropagation()} />
    </div>{(drop.dragOver || native.isDragOver) && <div data-testid="highlight" />}</div>;
  }
  const elementFromPoint = Object.getOwnPropertyDescriptor(document, "elementFromPoint");
  try {
    await act(async () => root.render(<Harness />));
    const composer = host.querySelector<HTMLElement>('[data-testid="chat-composer-shell"]')!;
    composer.getBoundingClientRect = () => ({ left: 0, right: 200, top: 0, bottom: 100 } as DOMRect);
    const file = new File(["opaque"], "IMG_4321.heic", { type: "image/heic" });
    const transfer = { types: ["Files"], files: [file], items: [{ kind: "file", type: file.type, getAsFile: () => file }] };
    for (const [index, target] of [...host.querySelectorAll<HTMLElement>("[data-point]")].entries()) {
      Object.defineProperty(document, "elementFromPoint", { configurable: true, value: () => index === 0 ? document.body : target });
      const over = new Event("dragover", { bubbles: true, cancelable: true }); Object.defineProperty(over, "dataTransfer", { value: transfer });
      await act(async () => { target.dispatchEvent(over); if (!web) emit({ type: "over", position: { x: 10, y: 10 } }); });
      expect(over.defaultPrevented).toBe(true);
      expect(host.querySelectorAll('[data-testid="highlight"]')).toHaveLength(1);
      if (!web) {
        await act(async () => emit({ type: "drop", paths: ["C:/Photos/IMG_4321.heic"], position: { x: 10, y: 10 } }));
        expect(host.querySelector('[data-testid="highlight"]')).toBeNull();
      }
      const drop = new Event("drop", { bubbles: true, cancelable: true }); Object.defineProperty(drop, "dataTransfer", { value: transfer });
      await act(async () => { target.dispatchEvent(drop); });
      expect(drop.defaultPrevented).toBe(true);
      expect(host.querySelector('[data-testid="highlight"]')).toBeNull();
      if (web) expect(upload).toHaveBeenCalledTimes(index + 1);
      else { expect(materialize).toHaveBeenCalledTimes(index + 1); expect(paths).toHaveBeenLastCalledWith(["C:/uploads/hash.heic"], ["C:/Photos/IMG_4321.heic"]); }
    }
    expect(upload).toHaveBeenCalledTimes(web ? 4 : 0);
    expect(error).not.toHaveBeenCalled();
    if (!web) {
      const modal = document.createElement("div"); document.body.append(modal);
      Object.defineProperty(document, "elementFromPoint", { configurable: true, value: () => modal });
      await act(async () => emit({ type: "drop", paths: ["C:/Photos/IMG_4321.heic"], position: { x: 10, y: 10 } }));
      expect(materialize).toHaveBeenCalledTimes(4); modal.remove();
    }
  } finally {
    await act(async () => root.unmount()); host.remove();
    nativeBridge.fileDrop = originalDrop; nativeBridge.materializeDroppedFiles = originalMaterialize;
    if (elementFromPoint) Object.defineProperty(document, "elementFromPoint", elementFromPoint); else delete (document as Partial<Document>).elementFromPoint;
  }
});

test("an empty WebKit preview can land as an item-only photo without a second upload", async () => {
  fixture.web = true;
  const upload = vi.fn<(files: File[]) => Promise<void>>(async () => {});
  const showError = vi.fn();
  function Surface() {
    const { handlers } = useWebFileDrop(() => {}, showError, upload);
    return <div {...handlers}><textarea aria-label="Composer" /></div>;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Surface />));
    const field = host.querySelector("textarea")!;
    const dispatch = async (type: string, transfer: object) => {
      const event = new Event(type, { bubbles: true, cancelable: true });
      Object.defineProperty(event, "dataTransfer", { value: transfer });
      await act(async () => field.dispatchEvent(event));
      expect(event.defaultPrevented).toBe(true);
    };
    await dispatch("dragover", { types: [], items: [], files: [] });
    expect(upload).not.toHaveBeenCalled();
    const photo = new File(["pixels"], "photo");
    await dispatch("drop", { types: ["public.jpeg"], items: [
      { kind: "file", type: "public.jpeg", getAsFile: () => photo },
    ], files: [] });
    expect(upload).toHaveBeenCalledOnce();
    expect(upload.mock.calls[0]?.[0]).toEqual([expect.objectContaining({ name: "photo.jpg" })]);
    await dispatch("drop", { types: ["public.image"], items: [], files: [] });
    expect(upload).toHaveBeenCalledOnce();
    expect(showError).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
  }
});
