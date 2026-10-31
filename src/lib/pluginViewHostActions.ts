/**
 * `ct.host.*` user-gesture actions a plugin view asks the app for.
 *
 * The iframe is untrusted: it names an action and its arguments, nothing else.
 * The app supplies everything that carries authority — the plugin id, the
 * document's private lease (stamped by `PluginDocument.hostRequest`) and the
 * user intent it observes itself (see `ViewHostIntent`). The daemon then
 * enforces the manifest declaration, the lease and media ownership. Every
 * outcome is a typed result; nothing throws into the frame, and no path ever
 * reaches it.
 */

export const VIEW_HOST_ACTIONS = ["copyText", "saveAs", "reveal", "pickFile"] as const;
export type ViewHostActionName = (typeof VIEW_HOST_ACTIONS)[number];
export type ViewHostRefusalCode = "denied" | "unavailable" | "invalid";
export type ViewHostRefusal = { ok: false; code: ViewHostRefusalCode; reason: string };
/** One file the user picked, as the view sees it: an opaque frame handle, never a path. */
export interface PickedInput { handle: string; fileName: string; contentType: string; size: number }
export type ViewHostResult =
  | { ok: true }
  | { ok: true; inputs: PickedInput[]; skipped: { fileName: string; code: ViewHostRefusalCode; reason: string }[] }
  | ViewHostRefusal;

/** Longest text a view may put on the clipboard in one call. */
export const MAX_COPY_TEXT = 100_000;
const MAX_FILE_NAME = 120;

export interface ViewHostMediaRequest {
  pluginId: string;
  handle: string;
}

/** A file the user chose in the app's own picker: the native dialog's path, or the browser's File. */
export type PickedFile = { kind: "path"; path: string; name: string } | { kind: "file"; file: File };

export interface ViewHostDeps {
  /** `POST /api/plugins/view-host-action` through the document's lease; the daemon's typed answer. */
  call(request: Record<string, unknown>): Promise<unknown>;
  /** The clipboard; absent where the page has none (an insecure web mirror origin). */
  writeText?: (text: string) => Promise<void>;
  /** The native save dialog; absent where the app cannot write a local file (web mirror). */
  pickDestination?: (suggestedName: string) => Promise<string | null>;
  /** Saves through the browser instead of a dialog; hands the file over only while `isCurrent()`. */
  download(request: ViewHostMediaRequest, fileName: string, isCurrent: () => boolean): Promise<ViewHostResult>;
  /** The native file manager; absent on the web mirror. */
  reveal?: (path: string) => Promise<void>;
  /** The app's own open picker (native dialog or a parent-document file input); null when dismissed. */
  pickFiles?: (accept: string[], multiple: boolean) => Promise<PickedFile[] | null>;
  /** `POST /api/plugins/view-input` through the document's lease; the daemon's typed answer. */
  ingest?: (pluginId: string, picked: PickedFile) => Promise<unknown>;
}

const refused = (code: ViewHostRefusalCode, reason: string): ViewHostRefusal => ({ ok: false, code, reason });

export type ViewHostSupport = Record<ViewHostActionName, boolean>;

/**
 * What this app instance can perform for a view, sent once when the frame is ready so a view can
 * hide an action instead of probing it. It grants nothing: declaration, gesture and media
 * ownership are still checked on every use.
 */
export function viewHostSupport(deps: Pick<ViewHostDeps, "writeText" | "reveal" | "pickFiles" | "ingest">): ViewHostSupport {
  return {
    copyText: deps.writeText !== undefined,
    saveAs: true,
    reveal: deps.reveal !== undefined,
    pickFile: deps.pickFiles !== undefined && deps.ingest !== undefined,
  };
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

type DaemonAnswer =
  | { ok: true; path?: string; body: Record<string, unknown> }
  | ViewHostRefusal;

/** Narrow whatever the daemon answered to the typed result; anything else is the host being unavailable. */
export function viewHostResult(value: unknown): DaemonAnswer {
  const body = record(value);
  if (body?.ok === true) return typeof body.path === "string" ? { ok: true, path: body.path, body } : { ok: true, body };
  const code = body?.code;
  if (body?.ok === false && (code === "denied" || code === "unavailable" || code === "invalid") && typeof body.reason === "string") {
    return refused(code, body.reason.slice(0, 64));
  }
  return refused("unavailable", "host");
}

/** The declared bounds the pickFile preflight answers. */
function pickBounds(body: Record<string, unknown>): { accept: string[]; maxBytes: number; maxFiles: number } | null {
  const accept = Array.isArray(body.accept) ? body.accept.filter((kind): kind is string => typeof kind === "string") : [];
  const { maxBytes, maxFiles } = body;
  if (!accept.length || !Number.isSafeInteger(maxBytes) || !Number.isSafeInteger(maxFiles)) return null;
  return { accept, maxBytes: maxBytes as number, maxFiles: maxFiles as number };
}

/** The ingest route's answer for one file, narrowed to what the view may see. */
function pickedInput(value: unknown): PickedInput | ViewHostRefusal {
  const answer = viewHostResult(value);
  if (!answer.ok) return answer;
  const input = record(answer.body.input);
  const { handle, fileName, contentType, size } = input ?? {};
  if (typeof handle !== "string" || !/^[a-f0-9]{64}$/.test(handle) || typeof fileName !== "string" ||
      typeof contentType !== "string" || !Number.isSafeInteger(size)) return refused("unavailable", "host");
  return { handle, fileName, contentType, size: size as number };
}

const pickedName = (picked: PickedFile) => picked.kind === "path" ? picked.name : picked.file.name;

/** A file name the user sees in a dialog or download: no separators, controls or reserved characters. */
export function safeFileName(raw: unknown): string {
  const cleaned = (typeof raw === "string" ? raw : "")
    .replace(/[\u0000-\u001f\u007f/\\:*?"<>|]/g, "")
    .trim()
    .replace(/^\.+/, "")
    .slice(0, MAX_FILE_NAME)
    .trim();
  return cleaned || "media";
}

/**
 * What the app itself observes when a request arrives: the view is shown, its own iframe holds
 * focus, and the app has transient user activation. This is the owner-accepted recent-activation
 * heuristic, not proof that one fresh click inside this frame asked for this effect: activation
 * lasts the browser's activation window and focus filters most, not all, foreign activation.
 * A boolean sent by the frame is never consulted. saveAs relies on the save dialog for intent.
 */
export interface ViewHostIntent {
  shown: boolean;
  frameFocused: boolean;
  activated: boolean;
}

export function intentGesture(intent: ViewHostIntent): boolean {
  return intent.shown && intent.frameFocused && intent.activated;
}

/** A rate limit, not a gesture detector: one request at a time per frame, at most one effect per interval. */
export const VIEW_HOST_COOLDOWN_MS = 1000;

export function createViewHostGate(now: () => number = () => Date.now()) {
  let busy = false;
  let quietUntil = 0;
  return {
    /** Claims the gate for one request; `false` when another is running or an effect just happened. */
    claim(): boolean {
      if (busy || now() < quietUntil) return false;
      busy = true;
      return true;
    },
    release(acted: boolean) {
      busy = false;
      if (acted) quietUntil = now() + VIEW_HOST_COOLDOWN_MS;
    },
  };
}

export type ViewHostGate = ReturnType<typeof createViewHostGate>;

/**
 * Performs one request. `isCurrent` is the app's fence for the asking frame: it is rechecked after
 * every await and before every effect (clipboard, dialog, daemon write, file manager, download), so
 * a frame closed, reloaded or replaced meanwhile gets `denied / stale` and nothing happens.
 */
export async function performViewHostAction(
  pluginId: string,
  message: { action?: unknown; args?: unknown },
  intent: ViewHostIntent,
  deps: ViewHostDeps,
  isCurrent: () => boolean,
  gate: ViewHostGate,
): Promise<ViewHostResult> {
  const action = VIEW_HOST_ACTIONS.find((name) => name === message.action);
  if (!action) return refused("invalid", "action");
  if (!intentGesture(intent)) return refused("denied", "gesture");
  if (!gate.claim()) return refused("denied", "busy");
  let acted = false;
  const stale = refused("denied", "stale");
  const args = record(message.args) ?? {};
  try {
    if (action === "copyText") {
      const text = args.text;
      if (typeof text !== "string" || text.length > MAX_COPY_TEXT) return refused("invalid", "text");
      const writeText = deps.writeText;
      if (!writeText) return refused("unavailable", "clipboard");
      const declared = viewHostResult(await deps.call({ pluginId, action }));
      if (!declared.ok) return declared;
      if (!isCurrent()) return stale;
      acted = true;
      try {
        await writeText(text);
      } catch {
        return refused("unavailable", "clipboard");
      }
      return { ok: true };
    }
    if (action === "pickFile") {
      const { pickFiles, ingest } = deps;
      if (!pickFiles || !ingest) return refused("unavailable", "pick");
      // Preflight: the declared kinds and bounds, before any dialog opens.
      const preflight = viewHostResult(await deps.call({ pluginId, action }));
      if (!preflight.ok) return preflight;
      const bounds = pickBounds(preflight.body);
      if (!bounds) return refused("unavailable", "host");
      // A view may only narrow what its manifest declared.
      const asked = Array.isArray(args.accept) ? args.accept.filter((kind) => bounds.accept.includes(kind as string)) as string[] : bounds.accept;
      if (!asked.length) return refused("invalid", "accept");
      const multiple = args.multiple === true && bounds.maxFiles > 1;
      if (!isCurrent()) return stale;
      acted = true;
      const picked = await pickFiles(asked, multiple);
      if (!picked?.length) return refused("denied", "cancelled");
      if (picked.length > (multiple ? bounds.maxFiles : 1)) return refused("invalid", "too_many");
      const inputs: PickedInput[] = [];
      const skipped: { fileName: string; code: ViewHostRefusalCode; reason: string }[] = [];
      for (const file of picked) {
        if (!isCurrent()) return stale;
        if (file.kind === "file" && file.file.size > bounds.maxBytes) {
          skipped.push({ fileName: safeFileName(file.file.name), code: "invalid", reason: "too_large" });
          continue;
        }
        const answer = pickedInput(await ingest(pluginId, file));
        if ("ok" in answer) skipped.push({ fileName: safeFileName(pickedName(file)), code: answer.code, reason: answer.reason });
        else inputs.push(answer);
      }
      if (!isCurrent()) return stale;
      if (!inputs.length) {
        const first = skipped[0];
        return first ? refused(first.code, first.reason) : refused("unavailable", "host");
      }
      return { ok: true, inputs, skipped };
    }
    const handle = args.handle;
    if (typeof handle !== "string" || !handle) return refused("invalid", "handle");
    const media = { pluginId, handle };
    if (action === "reveal") {
      if (!deps.reveal) return refused("unavailable", "reveal");
      const located = viewHostResult(await deps.call({ ...media, action }));
      if (!located.ok) return located;
      if (!located.path) return refused("unavailable", "host");
      if (!isCurrent()) return stale;
      acted = true;
      try {
        await deps.reveal(located.path);
      } catch {
        return refused("unavailable", "reveal_failed");
      }
      return { ok: true };
    }
    const name = safeFileName(args.suggestedName);
    if (!deps.pickDestination) {
      acted = true;
      return await deps.download(media, name, isCurrent);
    }
    // Preflight without a destination: only `invalid / destination` means the dialog is worth showing.
    const preflight = viewHostResult(await deps.call({ ...media, action }));
    if (preflight.ok || preflight.code !== "invalid" || preflight.reason !== "destination") {
      return preflight.ok ? refused("unavailable", "host") : preflight;
    }
    if (!isCurrent()) return stale;
    acted = true;
    const destination = await deps.pickDestination(name);
    if (!destination) return refused("denied", "cancelled");
    if (!isCurrent()) return stale;
    const saved = viewHostResult(await deps.call({ ...media, action, destination }));
    return saved.ok ? { ok: true } : saved;
  } catch {
    return refused("unavailable", "host");
  } finally {
    gate.release(acted);
  }
}
