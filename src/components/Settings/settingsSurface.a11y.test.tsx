// Run: npx tsx src/components/Settings/settingsSurface.a11y.test.tsx

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import { SettingsCard, SettingsPage } from "./shared";

/** Resolve an aria-labelledby the way an AT does: follow the id to its text. */
function labelledByText(html: string, container: RegExp): string | null {
  const id = html.match(container)?.[1];
  if (!id) return null;
  const escaped = id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const label = html.match(new RegExp(`id="${escaped}"[^>]*>([^<]*)<`));
  return label?.[1] ?? null;
}

const PAGE_REGION = /<div role="region" aria-labelledby="([^"]*)"/;
const CARD_GROUP = /aria-labelledby="([^"]*)"/;

function openingTag(html: string): string {
  return html.match(/<div[^>]*>/)?.[0] ?? "";
}

function page(title: string): string {
  return renderToStaticMarkup(
    <SettingsPage title={title} subtitle="subtitle">
      <span>body</span>
    </SettingsPage>,
  );
}

function card(title: string): string {
  return renderToStaticMarkup(<SettingsCard title={title}>body</SettingsCard>);
}

describe("settings page and card accessibility surface", () => {
  it("names each settings page from the title it was given", () => {
    for (const title of ["General", "Agents", "Check for updates"]) {
      expect(labelledByText(page(title), PAGE_REGION)).toBe(title);
    }
  });

  it("keeps the settings page addressable as a named region", () => {
    const html = page("Integrations");
    expect(html).toContain('role="region"');
    expect(html).toContain('data-testid="settings-page"');
  });

  it("names each settings card from the title it was given", () => {
    for (const title of ["Color Theme", "Keybindings", "Startup"]) {
      expect(labelledByText(card(title), CARD_GROUP)).toBe(title);
    }
  });

  it("gives two different cards two different names", () => {
    const theme = labelledByText(card("Color Theme"), CARD_GROUP);
    const font = labelledByText(card("Font"), CARD_GROUP);
    expect(theme).toBe("Color Theme");
    expect(font).toBe("Font");
  });

  it("keeps the card group role and its label on the card element itself", () => {
    const tag = openingTag(card("Font"));
    expect(tag).toContain('role="group"');
    expect(tag).toContain('data-testid="settings-card"');
    expect(tag).toContain("aria-labelledby=");
  });
});

finish();
