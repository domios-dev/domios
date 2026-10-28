import { describe, expect, it, vi } from "vitest";
import { MAX_COPY_TEXT, performViewHostAction, safeFileName, viewHostResult, viewHostSupport, type ViewHostDeps } from "./pluginViewHostActions";

const HANDLE = "a".repeat(64);

function deps(answers: unknown[] = [{ ok: true }], native = true) {
  const calls: Record<string, unknown>[] = [];
  const queue = [...answers];
  const fake = {
    call: vi.fn(async (request: Record<string, unknown>) => { calls.push(request); return queue.shift(); }),
    writeText: vi.fn(async () => undefined),
    pickDestination: native ? vi.fn(async (name: string): Promise<string | null> => `/home/me/Downloads/${name}`) : undefined,
    download: vi.fn(async () => ({ ok: true as const })),
    reveal: native ? vi.fn(async () => undefined) : undefined,
  } satisfies ViewHostDeps;
  return { fake, calls };
}

const run = (fake: ViewHostDeps, action: unknown, args: unknown, gesture = true) =>
  performViewHostAction("studio", "frame-1", { action, args }, gesture, fake);

describe("plugin view host actions", () => {
  it("refuses without the app's own user gesture before asking anything", async () => {
    const { fake, calls } = deps();
    expect(await run(fake, "copyText", { text: "hi" }, false)).toEqual({ ok: false, code: "denied", reason: "gesture" });
    expect(calls).toEqual([]);
    expect(fake.writeText).not.toHaveBeenCalled();
  });

  it("refuses an unknown action as invalid", async () => {
    const { fake, calls } = deps();
    expect(await run(fake, "openUrl", { url: "https://example.com" })).toEqual({ ok: false, code: "invalid", reason: "action" });
    expect(calls).toEqual([]);
  });

  it("copies text only after the daemon confirms the declaration", async () => {
    const { fake, calls } = deps([{ ok: true }]);
    expect(await run(fake, "copyText", { text: "a ceramic fox" })).toEqual({ ok: true });
    expect(calls).toEqual([{ pluginId: "studio", action: "copyText" }]);
    expect(fake.writeText).toHaveBeenCalledWith("a ceramic fox");

    const undeclared = deps([{ ok: false, code: "denied", reason: "undeclared" }]);
    expect(await run(undeclared.fake, "copyText", { text: "x" })).toEqual({ ok: false, code: "denied", reason: "undeclared" });
    expect(undeclared.fake.writeText).not.toHaveBeenCalled();
  });

  it("bounds copied text and reports a blocked clipboard", async () => {
    const { fake } = deps();
    expect(await run(fake, "copyText", { text: "x".repeat(MAX_COPY_TEXT + 1) })).toEqual({ ok: false, code: "invalid", reason: "text" });
    expect(await run(fake, "copyText", { text: 42 })).toEqual({ ok: false, code: "invalid", reason: "text" });
    fake.writeText.mockRejectedValueOnce(new Error("NotAllowedError"));
    expect(await run(fake, "copyText", { text: "x" })).toEqual({ ok: false, code: "unavailable", reason: "clipboard" });
  });

  it("saves through the native dialog only after a clean preflight, with the frame token the app holds", async () => {
    const { fake, calls } = deps([{ ok: false, code: "invalid", reason: "destination" }, { ok: true }]);
    expect(await run(fake, "saveAs", { handle: HANDLE, suggestedName: "../fox?.png" })).toEqual({ ok: true });
    expect(fake.pickDestination).toHaveBeenCalledWith("fox.png");
    expect(calls).toEqual([
      { pluginId: "studio", frame: "frame-1", handle: HANDLE, action: "saveAs" },
      { pluginId: "studio", frame: "frame-1", handle: HANDLE, action: "saveAs", destination: "/home/me/Downloads/fox.png" },
    ]);
  });

  it("never opens the save dialog for media the frame does not own", async () => {
    const { fake, calls } = deps([{ ok: false, code: "denied", reason: "media_foreign" }]);
    expect(await run(fake, "saveAs", { handle: HANDLE })).toEqual({ ok: false, code: "denied", reason: "media_foreign" });
    expect(fake.pickDestination).not.toHaveBeenCalled();
    expect(calls).toHaveLength(1);
  });

  it("reports a cancelled save dialog as denied", async () => {
    const { fake, calls } = deps([{ ok: false, code: "invalid", reason: "destination" }]);
    fake.pickDestination!.mockResolvedValueOnce(null);
    expect(await run(fake, "saveAs", { handle: HANDLE })).toEqual({ ok: false, code: "denied", reason: "cancelled" });
    expect(calls).toHaveLength(1);
  });

  it("downloads on the web mirror and refuses reveal there", async () => {
    const { fake, calls } = deps([], false);
    expect(await run(fake, "saveAs", { handle: HANDLE, suggestedName: "clip.mp4" })).toEqual({ ok: true });
    expect(fake.download).toHaveBeenCalledWith({ pluginId: "studio", frame: "frame-1", handle: HANDLE }, "clip.mp4");
    expect(await run(fake, "reveal", { handle: HANDLE })).toEqual({ ok: false, code: "unavailable", reason: "reveal" });
    expect(calls).toEqual([]);
  });

  it("reveals the path the daemon resolved, never one from the frame", async () => {
    const { fake, calls } = deps([{ ok: true, path: "/data/plugins/studio/media/1.png" }]);
    expect(await run(fake, "reveal", { handle: HANDLE, path: "/etc/passwd" })).toEqual({ ok: true });
    expect(fake.reveal).toHaveBeenCalledWith("/data/plugins/studio/media/1.png");
    expect(calls).toEqual([{ pluginId: "studio", frame: "frame-1", handle: HANDLE, action: "reveal" }]);
  });

  it("requires a handle for media actions and survives a failing host", async () => {
    const { fake } = deps();
    expect(await run(fake, "reveal", {})).toEqual({ ok: false, code: "invalid", reason: "handle" });
    fake.call.mockRejectedValueOnce(new Error("daemon down"));
    expect(await run(fake, "reveal", { handle: HANDLE })).toEqual({ ok: false, code: "unavailable", reason: "host" });
  });

  it("states what this app can perform so a view hides actions instead of probing them", async () => {
    expect(viewHostSupport(deps().fake)).toEqual({ copyText: true, saveAs: true, reveal: true });
    expect(viewHostSupport(deps([], false).fake)).toEqual({ copyText: true, saveAs: true, reveal: false });
    const { fake, calls } = deps();
    const noClipboard: ViewHostDeps = { ...fake, writeText: undefined };
    expect(viewHostSupport(noClipboard).copyText).toBe(false);
    expect(await run(noClipboard, "copyText", { text: "x" })).toEqual({ ok: false, code: "unavailable", reason: "clipboard" });
    expect(calls).toEqual([]);
  });

  it("narrows daemon answers and file names", () => {
    expect(viewHostResult({ ok: false, code: "root", reason: "x" })).toEqual({ ok: false, code: "unavailable", reason: "host" });
    expect(viewHostResult(null)).toEqual({ ok: false, code: "unavailable", reason: "host" });
    expect(safeFileName("..\\a\"b\n.mp4")).toBe("ab.mp4");
    expect(safeFileName("   ")).toBe("media");
    expect(safeFileName(undefined)).toBe("media");
  });
});
