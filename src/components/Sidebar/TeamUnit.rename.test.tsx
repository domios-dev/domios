import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
const [{ act }, { createRoot }, { TeamUnit }] = await Promise.all([
  import("react"),
  import("react-dom/client"),
  import("./TeamUnit"),
]);

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function renderRename(onRenameTeam: (teamId: string, name: string) => Promise<void>) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <TeamUnit
        team={{ id: "team-1", name: "Original Team", lifecycle: "active", created_at: 0, workspace_id: "ws" }}
        plan={null}
        panes={{}}
        managers={null}
        managerCount={1}
        workers={null}
        workerCount={0}
        workingWorkerCount={0}
        providers={[]}
        onRenameTeam={onRenameTeam}
        onOpenEffort={() => {}}
        onCloseTeam={() => {}}
      />,
    );
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
  await act(async () => trigger.click());
  const rename = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Rename")) as HTMLButtonElement;
  await act(async () => rename.click());
  const input = host.querySelector('[data-testid="team-name-input"]') as HTMLInputElement;
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    input.focus();
    setter?.call(input, "Renamed Team");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  return { host, root, input };
}

async function testEnterAndBlurInvokeOnce() {
  let invocations = 0;
  const { host, root, input } = await renderRename(async () => {
    invocations += 1;
  });
  await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  assert(invocations === 1, "immediate Enter and blur invoke rename exactly once");
  assert(host.querySelector('[data-testid="team-name"]')?.textContent === "Original Team", "rename has no optimistic local mutation");
  await act(async () => root.unmount());
}

async function testRenameRejectionStaysVisible() {
  const { host, root, input } = await renderRename(async () => {
    throw new Error("Team manager permission denied (403)");
  });
  await act(async () => input.blur());
  assert(host.querySelector('[data-testid="team-rename-error"]')?.textContent?.includes("403"), "rename rejection is visible");
  assert(host.querySelector('[data-testid="team-name-input"]'), "editor stays open after rejection");
  await act(async () => root.unmount());
}

await testEnterAndBlurInvokeOnce();
await testRenameRejectionStaysVisible();
console.log("TeamUnit rename tests passed");
}

void main();
