// Run: npx tsx src/components/AgentPicker/AgentProviderPicker.a11y.test.tsx

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentProviderPicker } from "./AgentProviderPicker";

function provider(overrides: Partial<ProviderDescriptor> = {}): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "claude",
    display_name: "Claude Code",
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    ...overrides,
  };
}

/** A provider enumeration observed as absent: the picker offers install cards. */
function missingCli(id: string, displayName: string): ProviderDescriptor {
  return provider({
    id,
    display_name: displayName,
    cli_binary: `${id}-cli`,
    availability: "unavailable",
  });
}

function render(headline: string, providers: ProviderDescriptor[]): string {
  return renderToStaticMarkup(
    <AgentProviderPicker headline={headline} providers={providers} onConfirm={() => undefined} />,
  );
}

function labelledByText(html: string): string | null {
  const id = html.match(/<div role="region" aria-labelledby="([^"]*)"/)?.[1];
  if (!id) return null;
  const escaped = id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return html.match(new RegExp(`id="${escaped}"[^>]*>([^<]*)<`))?.[1] ?? null;
}

function groupNames(html: string): string[] {
  return [...html.matchAll(/aria-label="([^"]*)"/g)].map((match) => match[1]);
}

describe("agent picker accessibility surface", () => {
  it("names the picker region from the headline it was given", () => {
    for (const headline of ["Pick your General Agent", "Choose a provider"]) {
      expect(labelledByText(render(headline, [provider()]))).toBe(headline);
    }
  });

  it("names each install card from the provider display name it was given", () => {
    const html = render("Pick your General Agent", [
      missingCli("claude", "Claude Code"),
      missingCli("codex", "Codex CLI"),
    ]);
    const names = groupNames(html);
    expect(names).toContain("Install Claude Code");
    expect(names).toContain("Install Codex CLI");
  });

  it("keeps install cards in the order they are shown, so focus follows the page", () => {
    const html = render("Pick your General Agent", [
      missingCli("codex", "Codex CLI"),
      missingCli("claude", "Claude Code"),
    ]);
    const claude = html.indexOf('aria-label="Install Claude Code"');
    const codex = html.indexOf('aria-label="Install Codex CLI"');
    expect(claude >= 0 && codex > claude).toBe(true);
  });

  it("offers a provider whose availability could not be established", () => {
    const html = render("Pick your General Agent", [
      provider({ id: "unknown-provider", display_name: "Unknown", cli_binary: "unknown-cli" }),
    ]);
    expect(html).toContain('data-availability="unknown"');
    expect(html).toContain('data-testid="agent-picker-cards"');
  });

  it("groups the selectable provider cards under a name of their own", () => {
    const html = render("Pick your General Agent", [provider()]);
    expect(html).toContain('aria-label="Agent providers"');
    expect(html).toContain('data-testid="agent-picker-cards"');
  });

  it("renders the provider cards in the order they were supplied", () => {
    const html = render("Pick your General Agent", [
      provider({ id: "opencode", display_name: "Opencode" }),
      provider({ id: "claude", display_name: "Claude Code" }),
    ]);
    const opencode = html.indexOf('data-testid="agent-picker-card-opencode"');
    const claude = html.indexOf('data-testid="agent-picker-card-claude"');
    expect(opencode >= 0 && claude > opencode).toBe(true);
  });
});

finish();
