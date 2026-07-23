// Run: npx tsx src/components/AgentPicker/preselectProvider.test.ts

import type { ProviderDescriptor } from "../../lib/types";
import {
  hasAnyInstalledProvider,
  isProviderInstalled,
  preselectProviderId,
  providerRequiresCli,
} from "./preselectProvider";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkProvider(id: string, cli?: string | null): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: id,
    transport: "pty",
    system_prompt_delivery: { cli_flag: "--system" },
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    cli_binary: cli ?? null,
    install_command: cli ? `install-${id}` : null,
  };
}

const providers = [mkProvider("claude", "claude"), mkProvider("codex", "codex")];

test("providerRequiresCli respects descriptor", () => {
  assert(providerRequiresCli(providers[0]), "claude has cli");
  assert(!providerRequiresCli(mkProvider("lm", null)), "no cli");
});

test("isProviderInstalled uses binary map", () => {
  assert(isProviderInstalled(providers[0], { claude: true }), "claude installed");
  assert(!isProviderInstalled(providers[0], { claude: false }), "claude missing");
});

test("preselect prefers claude when installed", () => {
  assert(
    preselectProviderId(providers, { claude: true, codex: true }) === "claude",
    "claude wins",
  );
});

test("preselect falls back to first installed", () => {
  assert(
    preselectProviderId(providers, { claude: false, codex: true }) === "codex",
    "codex when claude missing",
  );
});

test("preselect returns null when none installed", () => {
  assert(preselectProviderId(providers, { claude: false, codex: false }) === null, "none");
});

test("hasAnyInstalledProvider detects any", () => {
  assert(hasAnyInstalledProvider(providers, { codex: true }), "codex counts");
  assert(!hasAnyInstalledProvider(providers, { claude: false, codex: false }), "none");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`ok ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`FAIL ${name}:`, e);
  }
}
process.exit(failed > 0 ? 1 : 0);
