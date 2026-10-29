import { describe, expect, it, vi } from "vitest";
import {
  MAX_COPY_TEXT, VIEW_HOST_COOLDOWN_MS, createViewHostGate, performViewHostAction, safeFileName, viewHostResult, viewHostSupport,
  type ViewHostDeps, type ViewHostGate, type ViewHostIntent,
} from "./pluginViewHostActions";

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

const CLICK_IN_FRAME: ViewHostIntent = { shown: true, frameFocused: true, activated: true };

const run = (
  fake: ViewHostDeps,
  action: unknown,
  args: unknown,
  gesture: boolean | ViewHostIntent = true,
  isCurrent: () => boolean = () => true,
  gate: ViewHostGate = createViewHostGate(() => 0),
) => {
  const intent = gesture === true ? CLICK_IN_FRAME : gesture === false ? { ...CLICK_IN_FRAME, activated: false } : gesture;
  return performViewHostAction("studio", { action, args }, intent, fake, isCurrent, gate);
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const STALE = { ok: false, code: "denied", reason: "stale" };

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
      { pluginId: "studio", handle: HANDLE, action: "saveAs" },
      { pluginId: "studio", handle: HANDLE, action: "saveAs", destination: "/home/me/Downloads/fox.png" },
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
    expect(fake.download).toHaveBeenCalledWith({ pluginId: "studio", handle: HANDLE }, "clip.mp4", expect.any(Function));
    expect(await run(fake, "reveal", { handle: HANDLE })).toEqual({ ok: false, code: "unavailable", reason: "reveal" });
    expect(calls).toEqual([]);
  });

  it("reveals the path the daemon resolved, never one from the frame", async () => {
    const { fake, calls } = deps([{ ok: true, path: "/data/plugins/studio/media/1.png" }]);
    expect(await run(fake, "reveal", { handle: HANDLE, path: "/etc/passwd" })).toEqual({ ok: true });
    expect(fake.reveal).toHaveBeenCalledWith("/data/plugins/studio/media/1.png");
    expect(calls).toEqual([{ pluginId: "studio", handle: HANDLE, action: "reveal" }]);
  });

  it("requires a handle for media actions and survives a failing host", async () => {
    const { fake } = deps();
    expect(await run(fake, "reveal", {})).toEqual({ ok: false, code: "invalid", reason: "handle" });
    fake.call.mockRejectedValueOnce(new Error("daemon down"));
    expect(await run(fake, "reveal", { handle: HANDLE })).toEqual({ ok: false, code: "unavailable", reason: "host" });
  });

  it("states what this app can perform so a view hides actions instead of probing them", async () => {
    expect(viewHostSupport(deps().fake)).toEqual({ copyText: true, saveAs: true, reveal: true, pickFile: false });
    expect(viewHostSupport(deps([], false).fake)).toEqual({ copyText: true, saveAs: true, reveal: false, pickFile: false });
    expect(viewHostSupport({ ...deps().fake, pickFiles: async () => null, ingest: async () => null }).pickFile).toBe(true);
    const { fake, calls } = deps();
    const noClipboard: ViewHostDeps = { ...fake, writeText: undefined };
    expect(viewHostSupport(noClipboard).copyText).toBe(false);
    expect(await run(noClipboard, "copyText", { text: "x" })).toEqual({ ok: false, code: "unavailable", reason: "clipboard" });
    expect(calls).toEqual([]);
  });

  it("refuses recent activation unless the view is shown and its frame holds focus", async () => {
    const { fake, calls } = deps();
    // A click elsewhere still leaves the app activated, but focus is not in the asking frame.
    expect(await run(fake, "copyText", { text: "x" }, { shown: true, frameFocused: false, activated: true }))
      .toEqual({ ok: false, code: "denied", reason: "gesture" });
    expect(await run(fake, "copyText", { text: "x" }, { shown: false, frameFocused: true, activated: true }))
      .toEqual({ ok: false, code: "denied", reason: "gesture" });
    expect(calls).toEqual([]);
    expect(fake.writeText).not.toHaveBeenCalled();
  });

  it("does nothing once the frame is gone after the declaration check", async () => {
    const { fake } = deps();
    const answer = deferred<unknown>();
    fake.call.mockImplementationOnce(() => answer.promise);
    let current = true;
    const pending = run(fake, "copyText", { text: "x" }, true, () => current);
    current = false;
    answer.resolve({ ok: true });
    expect(await pending).toEqual(STALE);
    expect(fake.writeText).not.toHaveBeenCalled();
  });

  it("never opens the file manager or the dialog for a frame replaced during preflight", async () => {
    const reveal = deps();
    const located = deferred<unknown>();
    reveal.fake.call.mockImplementationOnce(() => located.promise);
    let current = true;
    const revealing = run(reveal.fake, "reveal", { handle: HANDLE }, true, () => current);
    current = false;
    located.resolve({ ok: true, path: "/data/1.png" });
    expect(await revealing).toEqual(STALE);
    expect(reveal.fake.reveal).not.toHaveBeenCalled();

    const save = deps();
    const preflight = deferred<unknown>();
    save.fake.call.mockImplementationOnce(() => preflight.promise);
    current = true;
    const saving = run(save.fake, "saveAs", { handle: HANDLE }, true, () => current);
    current = false;
    preflight.resolve({ ok: false, code: "invalid", reason: "destination" });
    expect(await saving).toEqual(STALE);
    expect(save.fake.pickDestination).not.toHaveBeenCalled();
  });

  it("drops the chosen destination when the frame is replaced while the dialog is open", async () => {
    const { fake, calls } = deps([{ ok: false, code: "invalid", reason: "destination" }]);
    const picked = deferred<string | null>();
    fake.pickDestination!.mockImplementationOnce(() => picked.promise);
    let current = true;
    const saving = run(fake, "saveAs", { handle: HANDLE }, true, () => current);
    await vi.waitFor(() => expect(fake.pickDestination).toHaveBeenCalled());
    current = false;
    picked.resolve("/home/me/fox.png");
    expect(await saving).toEqual(STALE);
    expect(calls).toHaveLength(1);
  });

  // A rate limit, not a gesture detector: once the interval passes, the same activation may act again.
  it("rate-limits effects: one call at a time and at most one effect per interval", async () => {
    let now = 0;
    const gate = createViewHostGate(() => now);
    const { fake } = deps([{ ok: true }, { ok: true }]);
    const answer = deferred<unknown>();
    fake.call.mockImplementationOnce(() => answer.promise);
    const first = run(fake, "copyText", { text: "one" }, true, () => true, gate);
    expect(await run(fake, "copyText", { text: "two" }, true, () => true, gate)).toEqual({ ok: false, code: "denied", reason: "busy" });
    answer.resolve({ ok: true });
    expect(await first).toEqual({ ok: true });
    expect(await run(fake, "copyText", { text: "three" }, true, () => true, gate)).toEqual({ ok: false, code: "denied", reason: "busy" });
    now = VIEW_HOST_COOLDOWN_MS;
    expect(await run(fake, "copyText", { text: "four" }, true, () => true, gate)).toEqual({ ok: true });
    expect(fake.writeText.mock.calls).toEqual([["one"], ["four"]]);
  });

  it("narrows daemon answers and file names", () => {
    expect(viewHostResult({ ok: false, code: "root", reason: "x" })).toEqual({ ok: false, code: "unavailable", reason: "host" });
    expect(viewHostResult(null)).toEqual({ ok: false, code: "unavailable", reason: "host" });
    expect(safeFileName("..\\a\"b\n.mp4")).toBe("ab.mp4");
    expect(safeFileName("   ")).toBe("media");
    expect(safeFileName(undefined)).toBe("media");
  });
});

describe("pickFile", () => {
  const DECLARED = { ok: true, accept: ["image/png", "image/jpeg"], maxBytes: 100, maxFiles: 2 };
  const input = (n: number) => ({ handle: String(n).repeat(64), fileName: `ref${n}.png`, contentType: "image/png", size: 10 });
  const pngPath = (name: string) => ({ kind: "path" as const, path: `/home/me/${name}`, name });

  function picker(answers: unknown[], picked: ReturnType<typeof pngPath>[] | null, native = true) {
    const base = deps(answers, native);
    const ingested: unknown[] = [];
    const queue = [...picked ? picked.map((_, i) => ({ ok: true, input: input(i + 1) })) : []];
    const fake = {
      ...base.fake,
      pickFiles: vi.fn(async (_accept: string[], _multiple: boolean) => picked),
      ingest: vi.fn(async (_plugin: string, file: unknown) => { ingested.push(file); return queue.shift(); }),
    };
    return { ...base, fake, ingested };
  }

  it("preflights the declaration, narrows the dialog to it and returns only opaque handles", async () => {
    const { fake, calls } = picker([DECLARED], [pngPath("fox.png"), pngPath("slate.png")]);
    const result = await run(fake, "pickFile", { accept: ["image/png", "image/svg+xml"], multiple: true });
    expect(calls).toEqual([{ pluginId: "studio", action: "pickFile" }]);
    expect(fake.pickFiles).toHaveBeenCalledWith(["image/png"], true);
    expect(result).toEqual({ ok: true, inputs: [input(1), input(2)], skipped: [] });
    expect(JSON.stringify(result)).not.toContain("/home/me");
  });

  it("never opens the picker for an undeclared view or a request outside the declaration", async () => {
    const undeclared = picker([{ ok: false, code: "denied", reason: "undeclared" }], [pngPath("fox.png")]);
    expect(await run(undeclared.fake, "pickFile", {})).toEqual({ ok: false, code: "denied", reason: "undeclared" });
    expect(undeclared.fake.pickFiles).not.toHaveBeenCalled();
    const outside = picker([DECLARED], [pngPath("fox.png")]);
    expect(await run(outside.fake, "pickFile", { accept: ["video/mp4"] })).toEqual({ ok: false, code: "invalid", reason: "accept" });
    expect(outside.fake.pickFiles).not.toHaveBeenCalled();
  });

  it("caps the pick at the declared count and treats a dismissed picker as cancelled", async () => {
    const single = picker([{ ...DECLARED, maxFiles: 1 }], [pngPath("fox.png")]);
    await run(single.fake, "pickFile", { multiple: true });
    expect(single.fake.pickFiles).toHaveBeenCalledWith(["image/png", "image/jpeg"], false);
    const many = picker([DECLARED], [pngPath("a.png"), pngPath("b.png"), pngPath("c.png")]);
    expect(await run(many.fake, "pickFile", { multiple: true })).toEqual({ ok: false, code: "invalid", reason: "too_many" });
    expect(many.ingested).toEqual([]);
    const dismissed = picker([DECLARED], null);
    expect(await run(dismissed.fake, "pickFile", {})).toEqual({ ok: false, code: "denied", reason: "cancelled" });
  });

  it("reports files the host refused next to the ones it took", async () => {
    const { fake } = picker([DECLARED], [pngPath("fox.png"), pngPath("notes.png")]);
    fake.ingest.mockResolvedValueOnce({ ok: true, input: input(1) }).mockResolvedValueOnce({ ok: false, code: "invalid", reason: "type" });
    expect(await run(fake, "pickFile", { multiple: true })).toEqual({
      ok: true, inputs: [input(1)], skipped: [{ fileName: "notes.png", code: "invalid", reason: "type" }],
    });
  });

  it("never ingests for a frame replaced while the picker was open", async () => {
    const { fake } = picker([DECLARED], [pngPath("fox.png")]);
    const chosen = deferred<ReturnType<typeof pngPath>[] | null>();
    fake.pickFiles.mockImplementationOnce(() => chosen.promise);
    let current = true;
    const picking = run(fake, "pickFile", {}, true, () => current);
    await vi.waitFor(() => expect(fake.pickFiles).toHaveBeenCalled());
    current = false;
    chosen.resolve([pngPath("fox.png")]);
    expect(await picking).toEqual(STALE);
    expect(fake.ingest).not.toHaveBeenCalled();
  });
});
