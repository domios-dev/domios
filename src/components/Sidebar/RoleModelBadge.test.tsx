import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { RoleModelBadge, shouldDropModelAtScale } from "./RoleModelBadge";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/role-model-badge" });
  for (const k of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLSpanElement", "Node", "Event", "CustomEvent", "SVGElement"] as const) {
    Object.defineProperty(globalThis, k, { value: (window as never)[k], configurable: true });
  }
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(ui); });
  return { host, root };
}

const SMALLEST = -1;
const DEFAULT = 1;
const LARGEST = 2;

test("smallest scale drops the MODEL, always keeps the ROLE", () => {
  assert(shouldDropModelAtScale(SMALLEST) === true, "model drops at the smallest scale");
  assert(shouldDropModelAtScale(DEFAULT) === false, "model kept at the default scale");
  assert(shouldDropModelAtScale(LARGEST) === false, "model kept at the largest scale");
});

test("ORCHESTRATOR · OPUS renders whole (role + model) at default and largest scales", async () => {
  for (const scale of [DEFAULT, LARGEST]) {
    const { host, root } = await render(
      <RoleModelBadge role="ORCHESTRATOR" model="OPUS" uiFontScale={scale} />,
    );
    const badge = host.querySelector<HTMLElement>('[data-testid="leadership-badge"]')!;
    assert(host.querySelector('[data-testid="role-badge-orchestrator"]'), `role chip present @${scale}`);
    assert(host.querySelector('[data-testid="leadership-model"]'), `model chip present @${scale}`);
    assert(badge.textContent?.includes("Orchestrator"), `role text present @${scale}`);
    assert(badge.textContent?.includes("OPUS"), `model text present @${scale}`);
    await act(async () => root.unmount());
  }
});

test("smallest scale keeps ORCHESTRATOR whole but drops OPUS (no truncation)", async () => {
  const { host, root } = await render(
    <RoleModelBadge role="ORCHESTRATOR" model="OPUS" uiFontScale={SMALLEST} />,
  );
  const badge = host.querySelector<HTMLElement>('[data-testid="leadership-badge"]')!;
  assert(host.querySelector('[data-testid="role-badge-orchestrator"]'), "role chip still present");
  assert(badge.textContent?.includes("Orchestrator"), "role text whole, not truncated");
  assert(!host.querySelector('[data-testid="leadership-model"]'), "model chip dropped at smallest scale");
  assert(!badge.textContent?.includes("OPUS"), "model text gone (dropped, not clipped)");
  await act(async () => root.unmount());
});

test("PLANNER · FABLE follows the same rule across all three scales", async () => {
  for (const scale of [SMALLEST, DEFAULT, LARGEST]) {
    const { host, root } = await render(
      <RoleModelBadge role="PLANNER" model="FABLE" uiFontScale={scale} />,
    );
    const badge = host.querySelector<HTMLElement>('[data-testid="leadership-badge"]')!;
    assert(host.querySelector('[data-testid="role-badge-planner"]'), `PLANNER chip present @${scale}`);
    assert(badge.textContent?.includes("Planner"), `PLANNER text whole @${scale}`);
    const hasModel = Boolean(host.querySelector('[data-testid="leadership-model"]'));
    assert(hasModel === (scale !== SMALLEST), `model present iff not smallest @${scale}`);
    await act(async () => root.unmount());
  }
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
