// Run: npx tsx src/components/AgentPicker/preselectProvider.test.ts

import type { ProviderAvailability, ProviderDescriptor } from "../../lib/types";
import {
  generalAgentProviders,
  hasAnySelectableProvider,
  isProviderSelectable,
  preselectProviderId,
  providerAvailability,
  providerRequiresCli,
} from "./preselectProvider";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkProvider(
  id: string,
  cli?: string | null,
  availability?: ProviderAvailability,
): ProviderDescriptor {
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
    availability,
  };
}

/**
 * The same four rows the Rust CLI test reads
 * (`PROVIDER_AVAILABILITY_FIXTURE` in cli/src/cmd/agent_tests.rs), so the two
 * surfaces cannot drift on what a verdict means.
 */
const FIXTURE = [
  mkProvider("fixture-available", "fixture-available-cli", "available"),
  mkProvider("fixture-unavailable", "fixture-unavailable-cli", "unavailable"),
  mkProvider("fixture-unknown", "fixture-unknown-cli", "unknown"),
  mkProvider("fixture-legacy", "fixture-legacy-cli"),
];

test("the picker reads the descriptor's own three-valued field", () => {
  assert(providerAvailability(FIXTURE[0]) === "available", "available row");
  assert(providerAvailability(FIXTURE[1]) === "unavailable", "unavailable row");
  assert(providerAvailability(FIXTURE[2]) === "unknown", "unknown row");
  assert(
    providerAvailability(FIXTURE[3]) === "unknown",
    "a row from a peer that never carried the field observed nothing",
  );
});

test("only the observed negative is unselectable", () => {
  assert(isProviderSelectable(FIXTURE[0]), "available launches");
  assert(!isProviderSelectable(FIXTURE[1]), "unavailable is blocked");
  assert(isProviderSelectable(FIXTURE[2]), "unknown is offered, not blocked");
  assert(isProviderSelectable(FIXTURE[3]), "an absent field is not a veto");
});

test("General agent eligibility excludes chat backends before rendering or preselection", () => {
  const chatBackend = {
    ...mkProvider("lmstudio", null, "available"),
    transport: "chat_backend" as const,
  };
  const providers = [
    mkProvider("claude", "claude", "unavailable"),
    mkProvider("codex", "codex", "available"),
  ];
  const eligible = generalAgentProviders([chatBackend, ...providers]);
  assert(eligible.length === 2, "only PTY providers remain eligible");
  assert(
    eligible.every((provider) => provider.transport === "pty"),
    "all eligible providers use PTY",
  );
  assert(
    preselectProviderId(eligible) === "codex",
    "a chat backend cannot be preselected as the General agent",
  );
});

test("providerRequiresCli respects descriptor", () => {
  assert(providerRequiresCli(mkProvider("claude", "claude")), "claude has cli");
  assert(!providerRequiresCli(mkProvider("lm", null)), "no cli");
});

test("preselect prefers claude when it can launch", () => {
  const providers = [
    mkProvider("claude", "claude", "available"),
    mkProvider("codex", "codex", "available"),
  ];
  assert(preselectProviderId(providers) === "claude", "claude wins");
});

test("preselect falls back to the first that can launch", () => {
  const providers = [
    mkProvider("claude", "claude", "unavailable"),
    mkProvider("codex", "codex", "available"),
  ];
  assert(preselectProviderId(providers) === "codex", "codex when claude is missing");
});

test("preselect returns null when every provider is unavailable", () => {
  const providers = [
    mkProvider("claude", "claude", "unavailable"),
    mkProvider("codex", "codex", "unavailable"),
  ];
  assert(preselectProviderId(providers) === null, "none");
});

test("hasAnySelectableProvider detects any", () => {
  assert(
    hasAnySelectableProvider([
      mkProvider("claude", "claude", "unavailable"),
      mkProvider("codex", "codex", "available"),
    ]),
    "codex counts",
  );
  assert(
    !hasAnySelectableProvider([
      mkProvider("claude", "claude", "unavailable"),
      mkProvider("codex", "codex", "unavailable"),
    ]),
    "none",
  );
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
