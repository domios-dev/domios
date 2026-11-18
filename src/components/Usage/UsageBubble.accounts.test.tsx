import assert from "node:assert/strict";
import { act, useLayoutEffect, useRef } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { UsageSnapshot } from "../../types/usage";

const dom = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: dom[key], configurable: true });
}

const bridge = await import("../../lib/bridge");
const { UsageBubble } = await import("./UsageBubble");
const { useProviderModelUsageState } = await import("../../hooks/useProviderModelUsage");
const requests: string[] = [];
let finishRead: (value: UsageSnapshot | null) => void = () => assert.fail("Expected a pending account read");
const rows = new Map<string, UsageSnapshot>([
  ["default", { provider: "fixture", account_id: null, session_pct: 12, parsed_ok: true, captured_at: Date.now() } as UsageSnapshot],
  ["work", { provider: "fixture", account_id: "work", session_pct: 73, parsed_ok: true, captured_at: Date.now() } as UsageSnapshot],
]);

bridge.__setListenForTest(async () => () => {});
bridge.__setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
  const account = String(args?.accountId ?? "default");
  if (command === "provider_model_usage") {
    return (rows.has(account) ? [{ group: account }] : []) as T;
  }
  if (command !== "usage_latest") return null as T;
  requests.push(account);
  if (account === "unknown") return new Promise<UsageSnapshot | null>(resolve => { finishRead = resolve; }) as Promise<T>;
  return rows.get(account) as T;
});

const contents = document.createElement("div");
document.body.append(contents);
const root = createRoot(contents);
const commits: (string | null)[] = [];

function Meter({ tabId, accountId }: { tabId: string; accountId: string | null }) {
  const element = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (tabId === "second") commits.push(element.current?.querySelector("button")?.getAttribute("aria-label") ?? null);
  });
  return <div ref={element} data-tab={tabId}>
    <UsageBubble provider="fixture" tabId={tabId} accountId={accountId} trackingEnabled />
  </div>;
}

function GroupReading({ accountId }: { accountId: string | null }) {
  const { usage } = useProviderModelUsageState("fixture", true, accountId);
  const output = useRef<HTMLOutputElement>(null);
  useLayoutEffect(() => {
    commits.push(output.current?.textContent ?? null);
  });
  return <output ref={output}>{usage.map(group => group.group).join(",")}</output>;
}

const render = (accountId: string | null) => act(async () => root.render(<>
  <Meter tabId="first" accountId={null} />
  <Meter tabId="second" accountId={accountId} />
  <GroupReading accountId={accountId} />
</>));
const label = (tab: string) => contents.querySelector(`[data-tab=${tab}] button`)?.getAttribute("aria-label") ?? null;

await render("work");
assert.equal(label("first"), "usage session 12%");
assert.equal(label("second"), "usage session 73%");
assert.equal(contents.querySelector("output")?.textContent, "work");
assert.deepEqual(requests, ["default", "work"]);

commits.length = 0;
await render("unknown");
assert.ok(commits.includes(null));
assert.ok(!commits.includes("usage session 73%") && !commits.includes("work"));
assert.equal(label("second"), null);
assert.equal(label("first"), "usage session 12%");
assert.equal(contents.querySelector("output")?.textContent, "");

await render(null);
assert.equal(label("second"), "usage session 12%");
assert.equal(contents.querySelector("output")?.textContent, "default");
await act(async () => finishRead(null));
assert.equal(label("second"), "usage session 12%");

await act(async () => root.unmount());
await dom.happyDOM.close();
console.log("Per-tab usage follows account switches and rejects late readings.");
