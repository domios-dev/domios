/**
 * One fixture for both notification surfaces.
 *
 * The bell panel and the toast render the same stored rows, so a claim proved
 * against one of them proves nothing about the other unless both are driven
 * from here. This module deliberately imports nothing that the separation
 * itself added, so the defect test built on it also runs against a tree that
 * predates the split.
 */
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { NotificationPanel } from "../NotificationPanel";
import { NotificationToastContainer } from "../NotificationToast";
import { __setInvokeForTest } from "../../../lib/bridge";
import type { Notification } from "../../../lib/types";

export type InvokeCall = { command: string; args: unknown };

export function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

export function setupDom(): void {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  const define = (name: string, value: unknown) =>
    Object.defineProperty(globalThis, name, { value, configurable: true });
  define("window", window);
  define("document", window.document);
  define("navigator", window.navigator);
  define("HTMLElement", window.HTMLElement);
  define("SVGElement", window.SVGElement);
  define("Element", window.Element);
  define("Node", window.Node);
  define("Event", window.Event);
  define("MouseEvent", window.MouseEvent);
  define("PointerEvent", window.PointerEvent);
  define("requestAnimationFrame", (cb: FrameRequestCallback) =>
    window.setTimeout(() => cb(0), 0),
  );
}

/** A row carrying whatever action set the caller of this fixture wants tested. */
export function row(actions: unknown[]): Notification {
  return {
    id: "notice-1",
    key: "notice-1",
    tab_id: null,
    workspace_id: null,
    title: "Deploy?",
    body: null,
    severity: "Info",
    timestamp: 1,
    read: false,
    sticky: true,
    actions: actions as Notification["actions"],
  };
}

/**
 * The shape a caller could put on the wire before actions carried daemon
 * provenance: an invoke naming any command it liked. Also exactly the shape of
 * a legacy stored row.
 */
export const CALLER_SUPPLIED_INVOKE = {
  type: "invoke",
  label: "Go",
  value: "clear_notifications",
  args: { id: "notice-1" },
};

export const NON_INVOKE_KINDS = [
  { type: "url", label: "Docs", value: "https://example.invalid" },
  { type: "dismiss", label: "Later" },
];

/** Every surface that renders a stored notification action. */
export const SURFACES = ["panel", "toast"] as const;
export type Surface = (typeof SURFACES)[number];

function element(
  surface: Surface,
  notification: Notification,
  onAnswer: (label: string) => void,
): React.ReactElement {
  if (surface === "panel") {
    return (
      <NotificationPanel
        notifications={[notification]}
        onMarkRead={() => {}}
        onMarkAllRead={() => {}}
        onDelete={() => {}}
        onClearAll={() => {}}
        onJumpToTab={() => {}}
        onAnswer={(_id, label) => onAnswer(label)}
        onClose={() => {}}
      />
    );
  }
  return <NotificationToastContainer toasts={[notification]} onDismiss={() => {}} />;
}

export interface Transcript {
  invoked: InvokeCall[];
  opened: string[];
  answered: string[];
  rendered: string[];
}

/**
 * Render one row on one surface, click every button carrying one of `labels`,
 * and report what crossed each outward boundary. The Tauri bridge is stubbed,
 * so "reached invoke" is a recorded fact rather than a guess at a side effect.
 */
export async function clickThrough(
  surface: Surface,
  notification: Notification,
  labels: string[],
): Promise<Transcript> {
  setupDom();
  const invoked: InvokeCall[] = [];
  const opened: string[] = [];
  const answered: string[] = [];
  __setInvokeForTest(async (command, args) => {
    invoked.push({ command, args });
    return undefined as never;
  });
  Object.defineProperty(window, "open", {
    value: (url?: string) => {
      opened.push(url ?? "");
      return null;
    },
    configurable: true,
  });
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(element(surface, notification, (label) => answered.push(label)));
  });
  const buttons = Array.from(host.querySelectorAll("button"));
  const rendered = buttons
    .map((button) => button.textContent ?? "")
    .filter((text) => labels.includes(text));
  for (const label of labels) {
    const button = buttons.find((candidate) => candidate.textContent === label);
    if (!button) continue;
    await act(async () => {
      (button as unknown as HTMLElement).click();
    });
  }
  await act(async () => root.unmount());
  __setInvokeForTest(null);
  return { invoked, opened, answered, rendered };
}

export function report(name: string, assertions: number): void {
  console.log(`${name}: ${assertions} assertions passed`);
}
