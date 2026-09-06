// The readers avoid importing the tone helper so they can detect divergence in any implementation.
import { renderToStaticMarkup } from "react-dom/server";
import type { Notification } from "../../lib/types";
import { ChatAskCard } from "./ChatAskCard";
import { NotificationPanel } from "./NotificationPanel";
import { NotificationToastContainer } from "./NotificationToast";

const ASKING_TAB = "tab-orchestrator";

export function record(overrides: Partial<Notification> = {}): Notification {
  return {
    id: "tone-axis-1",
    tab_id: ASKING_TAB,
    workspace_id: "ws-1",
    title: "Delete the stale win32 baselines?",
    body: "Left in place, the next run passes against baselines nobody reviewed.",
    severity: "Info",
    timestamp: 1_700_000_000_000,
    read: false,
    actions: [{ type: "answer", label: "Delete them" }],
    answer: { asked_from_tab: ASKING_TAB, answered: null, delivered: false },
    ...overrides,
  };
}

export function panelMarkup(notification: Notification): string {
  return renderToStaticMarkup(
    <NotificationPanel
      notifications={[notification]}
      onMarkRead={() => {}}
      onMarkAllRead={() => {}}
      onDelete={() => {}}
      onClearAll={() => {}}
      onJumpToTab={() => {}}
      onAnswer={() => {}}
      onClose={() => {}}
    />,
  );
}

export function cardMarkup(notification: Notification): string {
  return renderToStaticMarkup(<ChatAskCard notification={notification} onAnswer={() => {}} />);
}

export function toastMarkup(notification: Notification): string {
  return renderToStaticMarkup(
    <NotificationToastContainer toasts={[notification]} onDismiss={() => {}} />,
  );
}

function token(markup: string, pattern: RegExp, surface: string): string {
  const found = markup.match(pattern);
  if (!found) throw new Error(`no tone token found on the ${surface}`);
  return found[1];
}

/** The bell row's dot is the only inline background on the panel. */
export function panelTone(notification: Notification): string {
  return token(panelMarkup(notification), /background-color:\s*var\((--ct-[a-z-]+)\)/, "panel");
}

/** The chat card's edge is a color-mix over its tone token. */
export function cardTone(notification: Notification): string {
  return token(
    cardMarkup(notification),
    /border-color:color-mix\(in srgb,\s*var\((--ct-[a-z-]+)\)\s*28%/,
    "chat card",
  );
}

/** The toast hands its tone to the rest of its styling as one custom property. */
export function toastTone(notification: Notification): string {
  return token(toastMarkup(notification), /--toast-accent:\s*var\((--ct-[a-z-]+)\)/, "toast");
}

function attribute(markup: string, name: string): string | null {
  const found = markup.match(new RegExp(`${name}="([^"]*)"`));
  return found ? found[1] : null;
}

export function panelLevel(notification: Notification): string | null {
  return attribute(panelMarkup(notification), "data-notification-level");
}

export function cardLevel(notification: Notification): string | null {
  return attribute(cardMarkup(notification), "data-notification-level");
}

export function toastLevel(notification: Notification): string | null {
  return attribute(toastMarkup(notification), "data-notification-level");
}

export function toastSeverity(notification: Notification): string | null {
  return attribute(toastMarkup(notification), "data-notification-severity");
}
