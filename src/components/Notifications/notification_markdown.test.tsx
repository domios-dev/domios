// Run: npm run test:unit -- notification_markdown
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Notification } from "../../lib/types";
import { NotificationPanel } from "./NotificationPanel";
import { NotificationToastContainer } from "./NotificationToast";

type Test = [string, () => Promise<void> | void];

const tests: Test[] = [];

function test(name: string, run: Test[1]) {
  tests.push([name, run]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
}

function notification(body: string): Notification {
  return {
    id: "markdown-notification",
    title: "Rendered body",
    body,
    severity: "Info",
    timestamp: 1_700_000_000_000,
    read: false,
    sticky: true,
    tab_id: null,
    workspace_id: null,
  };
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

function assertFormatted(body: Element, surface: string) {
  assert(body.querySelector("strong")?.textContent === "bold", `${surface} must render bold`);
  assert(body.querySelector("code")?.textContent === "inline", `${surface} must render code`);
  assert(body.querySelectorAll("li").length === 2, `${surface} must render the list`);
  assert(!body.textContent?.includes("**"), `${surface} must not show emphasis markers`);
  assert(!body.textContent?.includes("`inline`"), `${surface} must not show code markers`);
}

const MARKDOWN = "**bold** with `inline` code\n\n- first\n- second";

test("notification_markdown bell panel renders the shared markdown result", async () => {
  setupDom();
  const item = notification(MARKDOWN);
  const { host, root } = await render(
    <NotificationPanel
      notifications={[item]}
      onMarkRead={() => {}}
      onMarkAllRead={() => {}}
      onDelete={() => {}}
      onClearAll={() => {}}
      onJumpToTab={() => {}}
      onClose={() => {}}
    />,
  );
  const toggle = Array.from(host.querySelectorAll("button")).find((button) =>
    button.getAttribute("aria-label")?.includes("complete message"),
  );
  assert(toggle, "the complete-message control must exist");
  if (toggle.getAttribute("aria-expanded") !== "true") {
    await act(async () => toggle.click());
  }
  const body = host.querySelector('[data-testid="notification-expanded-body"]');
  assert(body, "the expanded bell body must exist");
  assertFormatted(body, "bell panel");

  await act(async () => root.render(
    <NotificationPanel
      notifications={[notification("plain body unchanged")]}
      onMarkRead={() => {}}
      onMarkAllRead={() => {}}
      onDelete={() => {}}
      onClearAll={() => {}}
      onJumpToTab={() => {}}
      onClose={() => {}}
    />,
  ));
  assert(host.textContent?.includes("plain body unchanged"), "plain bell text must stay unchanged");
  await act(async () => root.unmount());
});

test("notification_markdown toast renders the shared markdown result", async () => {
  setupDom();
  const item = notification(MARKDOWN);
  const { host, root } = await render(
    <NotificationToastContainer toasts={[item]} onDismiss={() => {}} embedded />,
  );
  const toast = host.querySelector('[data-testid="notification-toast"]') as HTMLElement | null;
  assert(toast, "the toast must exist");
  await act(async () => toast.click());
  const expanded = document.querySelector('[data-testid="notification-toast-expanded"]');
  assert(expanded, "the expanded toast body must exist");
  assertFormatted(expanded, "toast");

  const plain = notification("plain body unchanged");
  await act(async () => root.render(
    <NotificationToastContainer toasts={[plain]} onDismiss={() => {}} embedded />,
  ));
  assert(host.textContent?.includes("plain body unchanged"), "plain toast text must stay unchanged");
  await act(async () => root.unmount());
});

test("notification_markdown surfaces import RichMarkdown instead of parsing", () => {
  for (const file of ["NotificationPanel.tsx", "NotificationToast.tsx"]) {
    const source = readFileSync(new URL(file, import.meta.url), "utf8");
    assert(
      /import \{ RichMarkdown \} from "@codeterm\/chat-ui";/.test(source),
      `${file} must import the shared renderer`,
    );
    assert(/<RichMarkdown text=/.test(source), `${file} must call the shared renderer`);
    assert(!/react-markdown|remark|marked|dangerouslySetInnerHTML/.test(source), `${file} must not parse markdown`);
  }
});

async function run() {
  for (const [name, body] of tests) {
    await body();
    console.log(`  ✓ ${name}`);
  }
  console.log(`\n✓ ${tests.length}/${tests.length} notification_markdown tests passed`);
}

void run();
