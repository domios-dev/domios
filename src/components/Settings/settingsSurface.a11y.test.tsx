// Run: npx tsx src/components/Settings/settingsSurface.a11y.test.tsx

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import { SettingsCard, SettingsPage, SettingsSection, SwitchTile, Toggle } from "./shared";

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

  it("names each toggle switch from the label it was given", () => {
    for (const label of ["Ligatures", "Copy on select", "Launch CodeTerm on system start"]) {
      const html = renderToStaticMarkup(
        <Toggle label={label} checked={false} onChange={() => undefined} />,
      );
      const name = html.match(/role="switch"[^>]*aria-label="([^"]*)"|aria-label="([^"]*)"[^>]*role="switch"/);
      expect(name?.[1] ?? name?.[2]).toBe(label);
    }
  });

  it("names each switch tile from the label it was given, never from its hint", () => {
    for (const label of ["Show the bell", "Notification sounds"]) {
      const html = renderToStaticMarkup(
        <SwitchTile
          label={label}
          hint="an explanation that is not the question"
          checked={false}
          onChange={() => undefined}
        />,
      );
      expect(html.match(/aria-label="([^"]*)"/)?.[1]).toBe(label);
      expect(html).toContain('role="switch"');
    }
  });

  it("gives a switch tile exactly one focusable control and hides the drawn switch", () => {
    const html = renderToStaticMarkup(
      <SwitchTile label="Desktop notifications" checked onChange={() => undefined} />,
    );
    expect((html.match(/<button/g) ?? []).length).toBe(1);
    expect((html.match(/role="switch"/g) ?? []).length).toBe(1);
    expect(html).toContain('aria-hidden="true"');
    expect(html).toContain('aria-checked="true"');
  });

  it("names each settings section from the title it was given", () => {
    for (const title of ["Surfaces", "What each level may interrupt"]) {
      const html = renderToStaticMarkup(
        <SettingsSection title={title}>
          <span>body</span>
        </SettingsSection>,
      );
      expect(labelledByText(html, /<section aria-labelledby="([^"]*)"/)).toBe(title);
    }
  });

  it("keeps the card group role and its label on the card element itself", () => {
    const tag = openingTag(card("Font"));
    expect(tag).toContain('role="group"');
    expect(tag).toContain('data-testid="settings-card"');
    expect(tag).toContain("aria-labelledby=");
  });
});

finish();
