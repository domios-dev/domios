import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TeamChannelAction, TeamChannelRow, TeamTemplatesTransport } from "../../lib/transport/teamTemplates";
import { publishTeamChannels } from "../../lib/teamTemplates/teamChannels";
import { TeamChannelsCard } from "./TeamChannelsCard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const official: TeamChannelRow = {
  name: "domios-teams", displayName: "Domios teams", description: "", url: "https://example.invalid/official.tar.gz", path: "/data/official",
  kind: "archive", official: true, lastRefresh: "ok",
};
const community: TeamChannelRow = {
  name: "acme", displayName: "Acme", description: "", url: "https://example.invalid/acme.git", path: "/data/acme", kind: "git", official: false,
  lastRefresh: "ok", skippedEntries: [
    { name: "acme/broken", code: "missing_package", reason: "opaque-os-detail" },
    { name: "acme/future", code: "requires_newer_domios", reason: "opaque-version-detail", requiredDomios: "9.9.9" },
  ],
};

let root: Root | null = null;
let host: HTMLDivElement;
let actions: TeamChannelAction[];
let failAdd: string | null;
let transport: () => TeamTemplatesTransport;
let channels: ReturnType<typeof vi.fn<() => Promise<{ channels: TeamChannelRow[] }>>>;

beforeEach(() => {
  actions = [];
  failAdd = null;
  publishTeamChannels([]);
  channels = vi.fn(async () => ({ channels: [official, community] }));
  const api = {
    channels,
    channelAction: vi.fn(async (action: TeamChannelAction) => {
      actions.push(action);
      if (action.action === "add" && failAdd) throw Object.assign(new Error("/api/team-channels → HTTP 400: {\"error\":\"trunc"), { body: failAdd });
      return { channels: action.action === "remove" ? [official] : [official, community] };
    }),
  };
  transport = () => api as unknown as TeamTemplatesTransport;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
});

async function render() {
  await act(async () => root!.render(<TeamChannelsCard transport={transport} />));
}

const rows = () => Array.from(host.querySelectorAll<HTMLElement>('[data-testid="team-channel-row"]'));
const row = (name: string) => rows().find((candidate) => candidate.dataset.channelName === name)!;
const labelled = (scope: ParentNode, label: string) => scope.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);

async function click(target: HTMLElement | null | undefined) {
  expect(target).toBeTruthy();
  await act(async () => target!.click());
}

async function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(input, value);
    input.dispatchEvent(new window.Event("input", { bubbles: true }));
  });
}

async function submitAdd(url: string, name = "") {
  await click(host.querySelector<HTMLButtonElement>('[data-testid="team-channel-add-open"]'));
  const form = host.querySelector<HTMLFormElement>('form[aria-label="Add channel"]')!;
  await type(form.querySelector<HTMLInputElement>('input[name="name"]')!, name);
  await type(form.querySelector<HTMLInputElement>('input[name="url"]')!, url);
  await act(async () => { form.requestSubmit(); });
}

describe("Team channels card", () => {
  it("lists the official channel without a remove control and community channels with refresh and remove", async () => {
    await render();
    expect(rows().map((candidate) => candidate.dataset.channelName)).toEqual(["domios-teams", "acme"]);
    expect(labelled(row("domios-teams"), "Remove Domios teams")).toBeNull();
    expect(labelled(row("domios-teams"), "Refresh Domios teams")).not.toBeNull();
    expect(labelled(row("acme"), "Remove Acme")).not.toBeNull();
    expect(labelled(row("acme"), "Refresh Acme")).not.toBeNull();
  });

  it("refreshes one channel by name and removes a community channel only after confirmation", async () => {
    await render();
    await click(labelled(row("acme"), "Refresh Acme"));
    expect(actions).toEqual([{ action: "refresh", name: "acme" }]);

    await click(labelled(row("acme"), "Remove Acme"));
    expect(actions).toHaveLength(1);
    const confirm = host.querySelector<HTMLElement>('[role="group"][aria-label="Remove Acme"]')!;
    await click(Array.from(confirm.querySelectorAll("button")).find((candidate) => candidate.textContent === "Cancel"));
    expect(actions).toHaveLength(1);

    await click(labelled(row("acme"), "Remove Acme"));
    const again = host.querySelector<HTMLElement>('[role="group"][aria-label="Remove Acme"]')!;
    await click(Array.from(again.querySelectorAll("button")).find((candidate) => candidate.textContent === "Remove"));
    expect(actions[actions.length - 1]).toEqual({ action: "remove", name: "acme" });
    expect(rows().map((candidate) => candidate.dataset.channelName)).toEqual(["domios-teams"]);
  });

  it("adds by URL with an optional name and explains a refused add inside the form", async () => {
    await render();
    await submitAdd("https://example.invalid/new.git");
    expect(actions).toEqual([{ action: "add", url: "https://example.invalid/new.git", name: null }]);
    expect(host.querySelector('form[aria-label="Add channel"]')).toBeNull();

    failAdd = JSON.stringify({ code: "folder_not_found", error: "opaque-plain-refusal" });
    await submitAdd("https://example.invalid/bad.git", "bad");
    expect(actions[actions.length - 1]).toEqual({ action: "add", url: "https://example.invalid/bad.git", name: "bad" });
    const alert = () => host.querySelector('form[aria-label="Add channel"] [role="alert"]')?.textContent ?? "";
    expect(alert()).toBe("opaque-plain-refusal");
    const url = host.querySelector<HTMLInputElement>('form[aria-label="Add channel"] input[name="url"]')!;
    expect(url.getAttribute("aria-invalid")).toBe("true");
    expect(document.getElementById(url.getAttribute("aria-describedby")!)?.textContent).toBe("opaque-plain-refusal");
    expect(host.querySelector('input[name="name"]')!.hasAttribute("aria-invalid")).toBe(false);

    failAdd = "opaque-raw-host-dump";
    await act(async () => { host.querySelector<HTMLFormElement>('form[aria-label="Add channel"]')!.requestSubmit(); });
    expect(alert()).not.toContain("opaque");
    expect(alert()).not.toContain("HTTP");
    expect(alert().length).toBeGreaterThan(0);
  });

  it("refuses an add that is not a channel address, or has no address, in its own words without calling the daemon", async () => {
    await render();
    await submitAdd("not a url");
    expect(actions).toEqual([]);
    const form = host.querySelector<HTMLFormElement>('form[aria-label="Add channel"]')!;
    expect(form.querySelector('[role="alert"]')).not.toBeNull();
    expect(form.noValidate).toBe(true);
    await type(form.querySelector<HTMLInputElement>('input[name="url"]')!, "");
    await act(async () => { form.requestSubmit(); });
    expect(actions).toEqual([]);
    expect(form.querySelector('[role="alert"]')?.textContent).toBeTruthy();
  });

  it("returns focus to Add channel when the form is cancelled", async () => {
    await render();
    await click(host.querySelector<HTMLButtonElement>('[data-testid="team-channel-add-open"]'));
    const cancel = host.querySelector<HTMLButtonElement>('[data-testid="team-channel-add-cancel"]')!;
    cancel.focus();
    await click(cancel);
    expect(host.querySelector('form[aria-label="Add channel"]')).toBeNull();
    expect(document.activeElement).toBe(host.querySelector('[data-testid="team-channel-add-open"]'));
  });

  it("says plainly when the official channel brings nothing for this Domios", async () => {
    const skipping: TeamChannelRow = { ...official, usableTeams: 0, skippedEntries: [
      { name: "dev.domios/default", code: "requires_newer_domios", reason: "opaque", requiredDomios: "1.20.0" },
    ] };
    channels.mockResolvedValueOnce({ channels: [skipping] });
    await render();
    expect(row("domios-teams").textContent).toContain("1.20.0");
    expect(row("domios-teams").textContent).toContain("bundled");
    expect(row("domios-teams").textContent).not.toContain("up to date");
  });

  it("reveals skipped entries on demand in words from their code, keeping the host's raw detail out of the text", async () => {
    await render();
    const toggle = Array.from(row("acme").querySelectorAll<HTMLButtonElement>("button[aria-expanded]"))[0];
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const list = document.getElementById(toggle.getAttribute("aria-controls")!)!;
    const items = Array.from(list.querySelectorAll("li"));
    expect(items.map((item) => item.getAttribute("title"))).toEqual(["opaque-os-detail", "opaque-version-detail"]);
    expect(list.textContent).not.toContain("opaque-");
    expect(items.every((item) => item.getAttribute("aria-label") && !item.getAttribute("aria-label")!.includes("opaque-"))).toBe(true);
    expect(items[1].textContent).toContain("9.9.9");
  });
});
