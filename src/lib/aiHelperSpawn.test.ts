import { AI_HELPER_SPAWN_PATH, spawnAiHelper } from "./aiHelperSpawn";
import { configureWithAI } from "./pluginConfigure";
import { startWebAccessSetupHelper } from "./webAccess";

let passed = 0;
let failed = 0;

function assert(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

type Captured = { url: string; body: Record<string, unknown> };

const g = globalThis as unknown as Record<string, unknown>;
const focused: string[] = [];
g.window = {
  dispatchEvent: (event: { type: string; detail?: { tabId?: string } }) => {
    if (event.type === "codeterm:focus-tab" && event.detail?.tabId) focused.push(event.detail.tabId);
    return true;
  },
};
g.CustomEvent = class<T> {
  type: string;
  detail: T;
  constructor(type: string, init?: { detail: T }) {
    this.type = type;
    this.detail = init?.detail as T;
  }
};

let captured: Captured[] = [];
function respond(status: number, payload: unknown) {
  g.fetch = async (url: string, init: { body?: string }) => {
    captured.push({ url, body: JSON.parse(init.body ?? "{}") });
    return { ok: status < 400, status, json: async () => payload };
  };
}

const api = { host: "127.0.0.1", port: 7686, token: "fixture-token" };
const opaqueTask = "ai-helper-task-token-4d21";
const opaquePort = 48213;

const buttons: Array<{ label: string; run: () => Promise<unknown>; carries: string }> = [
  {
    label: "Configure with AI",
    run: () => configureWithAI("fixture-plugin", "Fixture", opaqueTask, api),
    carries: opaqueTask,
  },
  {
    label: "Mesh Set up with AI",
    run: () => startWebAccessSetupHelper(null, opaquePort, api),
    carries: String(opaquePort),
  },
  {
    label: "shared helper",
    run: () => spawnAiHelper({ title: "fixture-title", task: opaqueTask }, api),
    carries: opaqueTask,
  },
];

async function run() {
  for (const button of buttons) {
    console.log(`\n${button.label}:`);
    captured = [];
    focused.length = 0;
    respond(200, { id: "tab-helper-91", workspace_id: "ws-general", delivery_state: "launched" });
    await button.run();
    const request = captured[0];
    assert("posts once to the daemon helper route", captured.length === 1 && request.url.endsWith(AI_HELPER_SPAWN_PATH));
    assert(
      "leaves provider, workspace and cwd to the daemon",
      JSON.stringify(Object.keys(request.body).sort()) === JSON.stringify(["task", "title"]),
    );
    assert("delivers its supplied content in the task", String(request.body.task).includes(button.carries));
    assert("focuses the spawned tab", focused.length === 1 && focused[0] === "tab-helper-91");

    const exactFailure = `fixture launch refused ${button.label.length}`;
    respond(503, { error: { message: exactFailure } });
    focused.length = 0;
    let surfaced = "";
    await button.run().catch((error: unknown) => {
      surfaced = error instanceof Error ? error.message : String(error);
    });
    assert("surfaces the exact daemon failure", surfaced === exactFailure);
    assert("does not focus a tab after a failure", focused.length === 0);
  }

  console.log("\nresponse contract:");
  respond(200, { id: "tab-confirmed", delivery_state: "confirmed" });
  const confirmed = await spawnAiHelper({ title: "t", task: opaqueTask }, api);
  assert("accepts evidence-backed confirmed delivery", confirmed.deliveryState === "confirmed");

  const rejects = async (payload: unknown) => {
    respond(200, payload);
    focused.length = 0;
    return spawnAiHelper({ title: "t", task: opaqueTask }, api).then(
      () => false,
      () => focused.length === 0,
    );
  };
  assert("refuses a success without a tab id", await rejects({ delivery_state: "launched" }));
  assert("refuses an unknown delivery state", await rejects({ id: "tab-x", delivery_state: "fixture-state" }));
  assert("refuses a missing delivery state", await rejects({ id: "tab-x" }));

  respond(500, null);
  let statusFailure = "";
  await spawnAiHelper({ title: "t", task: opaqueTask }, api).catch((error: unknown) => {
    statusFailure = error instanceof Error ? error.message : String(error);
  });
  assert("names the HTTP status when the body is not JSON", statusFailure.includes("500"));
}

run()
  .catch((e) => {
    console.error(e);
    failed++;
  })
  .finally(() => {
    console.log(`\n${passed} passed, ${failed} failed`);
    if (failed > 0) process.exit(1);
  });
