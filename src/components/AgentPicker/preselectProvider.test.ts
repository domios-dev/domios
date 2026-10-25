// Run: npx tsx src/components/AgentPicker/preselectProvider.test.ts

import { readFileSync } from "node:fs";
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
    general_agent_offer_when_absent: true,
    cli_binary: cli ?? null,
    install_command: cli ? `install-${id}` : null,
    availability,
  };
}

interface AvailabilityFixtureRow {
  id: string;
  availability?: ProviderAvailability;
}

const FIXTURE = (JSON.parse(
  readFileSync(
    new URL("../../../fixtures/provider_availability.json", import.meta.url),
    "utf8",
  ),
) as AvailabilityFixtureRow[]).map((row) =>
  mkProvider(row.id, `${row.id}-cli`, row.availability),
);

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

test("General agent eligibility follows PTY or agent runtime capabilities", () => {
  const runtime = {
    ...mkProvider("runtime-agent", "runtime-cli", "available"),
    transport: "chat_backend" as const,
    agent_runtime: {},
  };
  const missingRuntime = { ...runtime, id: "missing-runtime", availability: "unavailable" as const };
  const pty = mkProvider("terminal-agent", "terminal-cli", "available");
  const missingPty = mkProvider("missing-terminal", "terminal-cli", "unavailable");
  const plainChat = { ...mkProvider("plain-chat", null, "available"), transport: "chat_backend" as const };
  const remote = {
    ...runtime, id: "remote-task",
    remote_task: {
      backend: "remote", mode: "cloud", displayName: "Cloud", environments: [],
      availability: {status: "available" as const}, followUp: false, cancel: false, applyDiff: false,
    },
  };
  const eligible = generalAgentProviders([missingPty, runtime, pty, missingRuntime, plainChat, remote]);
  assert(
    eligible.map((provider) => provider.id).join(",") === "missing-terminal,runtime-agent,terminal-agent,missing-runtime,remote-task",
    "Local runtime eligibility remains valid when the provider also offers remote tasks",
  );
  assert(!isProviderSelectable(missingPty), "missing PTY remains install-only");
  assert(!isProviderSelectable(missingRuntime), "missing runtime remains install-only");
  assert(
    preselectProviderId(eligible) === runtime.id,
    "an available runtime can be preselected ahead of PTY providers",
  );
});

test("General offers bundled defaults always and Cursor only when detected", () => {
  for (const availability of ["available", "unavailable", "unknown", undefined] as const) {
    const providers = ["codex", "claude", "opencode", "cursor"].map((id) => {
      const manifest = JSON.parse(readFileSync(
        new URL(`../../../plugins/provider-${id}/plugin.json`, import.meta.url), "utf8",
      )) as { generalAgentOfferWhenAbsent?: boolean };
      return {
        ...mkProvider(id, `${id}-cli`, availability),
        general_agent_offer_when_absent: manifest.generalAgentOfferWhenAbsent,
        ...(id === "codex" ? { transport: "chat_backend" as const, agent_runtime: {} } : {}),
      };
    });
    const expected = availability === "available" ? "codex,claude,opencode,cursor" : "codex,claude,opencode";
    assert(generalAgentProviders(providers).map((provider) => provider.id).join(",") === expected,
      `General options for ${availability ?? "unobserved"}`);
  }
});

test("General offer policy belongs to capabilities rather than provider identities", () => {
  const hidden = { ...mkProvider("codex", "engine", "unavailable"), general_agent_offer_when_absent: false };
  const offered = { ...mkProvider("arbitrary-installable-agent", "engine", "unavailable"), general_agent_offer_when_absent: true };
  const plainChat = { ...offered, id: "plain-chat", transport: "chat_backend" as const };
  assert(generalAgentProviders([hidden, offered, plainChat]).map((provider) => provider.id).join(",") === offered.id,
    "the flag allows install offers only for eligible agent transports");
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
