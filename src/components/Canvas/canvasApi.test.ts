import type { ApiConfig, ProviderDescriptor } from "../../lib/types";
import { resolveCanvasSpawnProvider, spawnOrchestrator } from "./canvasApi";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function provider(id: string, transport: ProviderDescriptor["transport"]): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport,
    system_prompt_delivery: "external",
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    preserves_context_on_resume: false,
  };
}

async function run() {
  const originalFetch = globalThis.fetch;
  const payloads: unknown[] = [];
  globalThis.fetch = (async (_input, init) => {
    payloads.push(JSON.parse(String(init?.body)));
    return new Response(JSON.stringify({ pane_id: "orchestrator-1" }), { status: 200 });
  }) as typeof fetch;
  try {
    const selected = resolveCanvasSpawnProvider("ui-selected", [provider("ui-selected", "pty")]);
    assert(selected === "ui-selected", "uses the selected provider from the registry");
    await spawnOrchestrator({} as ApiConfig, { providerId: selected, workspaceId: "workspace-1" });
    const payload = payloads[0] as Record<string, unknown>;
    assert(payload.agent === "ui-selected", "payload carries the UI-selected provider through the accepted agent field");
    assert(payload.role === "Orchestrator", "payload carries the explicit Orchestrator role");
    assert(payload.workspace_id === "workspace-1", "payload carries workspace provenance");
    assert(!("task" in payload) && !("provider" in payload) && !("bootstrap" in payload), "payload contains only accepted provenance fields");

    const chatProvider = resolveCanvasSpawnProvider("chat-default", [provider("chat-default", "chat_backend")]);
    await spawnOrchestrator({} as ApiConfig, { providerId: chatProvider, workspaceId: "workspace-2" });
    assert((payloads[1] as Record<string, unknown>).agent === "chat-default", "chat backend defaults remain provider-neutral");
  } finally {
    globalThis.fetch = originalFetch;
  }
  console.log("canvasApi tests passed");
}

void run();
