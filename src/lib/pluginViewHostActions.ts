/**
 * `ct.host.*` user-gesture actions a plugin view asks the app for.
 *
 * The iframe is untrusted: it names an action and its arguments, nothing else.
 * The app supplies everything that carries authority — the plugin id, the
 * host-held frame token and the user gesture, read from the app's own
 * `navigator.userActivation` (a click inside the frame activates its ancestors;
 * the frame cannot forge it). The daemon then enforces the manifest declaration
 * and media ownership. Every outcome is a typed result; nothing throws into the frame.
 */

export const VIEW_HOST_ACTIONS = ["copyText", "saveAs", "reveal"] as const;
export type ViewHostActionName = (typeof VIEW_HOST_ACTIONS)[number];
export type ViewHostRefusalCode = "denied" | "unavailable" | "invalid";
export type ViewHostResult = { ok: true } | { ok: false; code: ViewHostRefusalCode; reason: string };

/** Longest text a view may put on the clipboard in one gesture. */
export const MAX_COPY_TEXT = 100_000;
const MAX_FILE_NAME = 120;

export interface ViewHostMediaRequest {
  pluginId: string;
  frame: string | null;
  handle: string;
}

export interface ViewHostDeps {
  /** `POST /api/plugins/view-host-action`; the daemon's typed answer. */
  call(request: Record<string, unknown>): Promise<unknown>;
  writeText(text: string): Promise<void>;
  /** The native save dialog; absent where the app cannot write a local file (web mirror, relayed peer). */
  pickDestination?: (suggestedName: string) => Promise<string | null>;
  /** Saves through the browser instead of a dialog. */
  download(request: ViewHostMediaRequest, fileName: string): Promise<ViewHostResult>;
  /** The native file manager; absent on the web mirror and a relayed peer. */
  reveal?: (path: string) => Promise<void>;
}

const refused = (code: ViewHostRefusalCode, reason: string): ViewHostResult => ({ ok: false, code, reason });

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

/** Narrow whatever the daemon answered to the typed result; anything else is the host being unavailable. */
export function viewHostResult(value: unknown): ViewHostResult & { path?: string } {
  const body = record(value);
  if (body?.ok === true) return typeof body.path === "string" ? { ok: true, path: body.path } : { ok: true };
  const code = body?.code;
  if (body?.ok === false && (code === "denied" || code === "unavailable" || code === "invalid") && typeof body.reason === "string") {
    return refused(code, body.reason.slice(0, 64));
  }
  return refused("unavailable", "host");
}

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

export async function performViewHostAction(
  pluginId: string,
  frame: string | null,
  message: { action?: unknown; args?: unknown },
  gesture: boolean,
  deps: ViewHostDeps,
): Promise<ViewHostResult> {
  const action = VIEW_HOST_ACTIONS.find((name) => name === message.action);
  if (!action) return refused("invalid", "action");
  if (!gesture) return refused("denied", "gesture");
  const args = record(message.args) ?? {};
  try {
    if (action === "copyText") {
      const text = args.text;
      if (typeof text !== "string" || text.length > MAX_COPY_TEXT) return refused("invalid", "text");
      const declared = viewHostResult(await deps.call({ pluginId, action }));
      if (!declared.ok) return declared;
      try {
        await deps.writeText(text);
      } catch {
        return refused("unavailable", "clipboard");
      }
      return { ok: true };
    }
    const handle = args.handle;
    if (typeof handle !== "string" || !handle) return refused("invalid", "handle");
    const media = { pluginId, frame, handle };
    if (action === "reveal") {
      if (!deps.reveal) return refused("unavailable", "reveal");
      const located = viewHostResult(await deps.call({ ...media, action }));
      if (!located.ok) return located;
      if (!located.path) return refused("unavailable", "host");
      try {
        await deps.reveal(located.path);
      } catch {
        return refused("unavailable", "reveal_failed");
      }
      return { ok: true };
    }
    const name = safeFileName(args.suggestedName);
    if (!deps.pickDestination) return await deps.download(media, name);
    // Preflight without a destination: only `invalid / destination` means the dialog is worth showing.
    const preflight = viewHostResult(await deps.call({ ...media, action }));
    if (preflight.ok || preflight.code !== "invalid" || preflight.reason !== "destination") {
      return preflight.ok ? refused("unavailable", "host") : preflight;
    }
    const destination = await deps.pickDestination(name);
    if (!destination) return refused("denied", "cancelled");
    const saved = viewHostResult(await deps.call({ ...media, action, destination }));
    return saved.ok ? { ok: true } : saved;
  } catch {
    return refused("unavailable", "host");
  }
}
