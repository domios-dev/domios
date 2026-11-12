import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import exchange from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/exchange.json";
import type { TeamTemplatesTransport, TemplateDistillResult } from "../../lib/transport/teamTemplates";
import { SaveTemplateDialog } from "./SaveTemplateDialog";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const review = exchange.distill_result as TemplateDistillResult;
const distill = vi.fn();
const exportPackage = vi.fn();
const transport = () => ({ distill, export: exportPackage } as unknown as TeamTemplatesTransport);
let root: Root | null = null;
let host: HTMLDivElement;
let onClose = vi.fn();
let onSaved = vi.fn();

beforeEach(() => {
  distill.mockReset();
  exportPackage.mockReset();
  distill.mockImplementation(async (request: { dry_run?: boolean }) => request.dry_run ? { ...review, saved: null, path: null } : review);
  onClose = vi.fn();
  onSaved = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
});

const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]')!;
const button = (text: string) => Array.from(dialog().querySelectorAll<HTMLButtonElement>("button")).find((candidate) => candidate.textContent?.trim() === text);

async function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(input, value);
    input.dispatchEvent(new window.Event("input", { bubbles: true }));
  });
}

async function renderDialog() {
  await act(async () => {
    root!.render(<SaveTemplateDialog teamId="team_7" teamName="Research desk" onClose={onClose} onSaved={onSaved} transport={transport} />);
  });
}

describe("Save as template", () => {
  it("reviews the team before writing anything, with its includes and what was removed", async () => {
    await renderDialog();
    expect(distill).toHaveBeenCalledOnce();
    expect(distill.mock.calls[0][0]).toMatchObject({ team_id: "team_7", dry_run: true });
    // The skill's script reads relative to the skill folder; View still opens the full path.
    expect(dialog().querySelector('[data-payload-item="skill:source-check"] [data-executable="true"]')?.textContent).toContain("Runs scripts/fetch.py");
    expect(dialog().querySelector('[data-payload-item="file:notes/brief.md"]')?.textContent).toContain("used by Orchestrator, Researcher");
    expect(dialog().querySelector('[aria-label="Removed before saving"]')?.textContent).toContain("SEARCH_API_KEY kept as a name");
    const [, description] = Array.from(dialog().querySelectorAll<HTMLInputElement>("input"));
    await type(description, "Researches a market question.");
    await act(async () => button("Show team.yaml")!.click());
    expect(dialog().querySelector('[aria-label="team.yaml"]')?.textContent).toBe(review.yaml);
  });

  it("starts What it does empty and still reviews, because the dry run alone uses a default", async () => {
    distill.mockImplementation(async (request: { description: string; dry_run?: boolean }) => {
      if (!request.description.trim()) throw new Error("invalid_package: description cannot be empty");
      return request.dry_run ? { ...review, saved: null, path: null } : review;
    });
    await renderDialog();
    const [, description] = Array.from(dialog().querySelectorAll<HTMLInputElement>("input"));
    expect(description.value).toBe("");
    expect(description.placeholder).toBe("One line on what this team does");
    expect(distill.mock.calls[0][0].description).not.toBe("");
    expect(dialog().querySelector('[role="alert"]')).toBeNull();
    expect(dialog().querySelector('[aria-label="Includes"]')).not.toBeNull();
    expect(button("Save template")!.disabled).toBe(true);
    await type(description, "Researches a market question.");
    await act(async () => button("Save template")!.click());
    expect(distill.mock.calls[distill.mock.calls.length - 1][0]).toMatchObject({ description: "Researches a market question.", save: true });
    expect(onSaved).toHaveBeenCalledExactlyOnceWith(review.saved);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("shows a refused review and recovers once the owner fixes the field", async () => {
    distill.mockRejectedValueOnce(new Error("invalid_package: version is not semver"));
    await renderDialog();
    expect(dialog().querySelector('[role="alert"]')?.textContent).toContain("version is not semver");
    expect(button("Save template")!.disabled).toBe(true);
    const [, description, version] = Array.from(dialog().querySelectorAll<HTMLInputElement>("input"));
    await type(version, "1.0.1");
    expect(dialog().querySelector('[role="alert"]')).toBeNull();
    await type(description, "Researches a market question.");
    expect(button("Save template")!.disabled).toBe(false);
  });

  it("says which field blocks Save", async () => {
    await renderDialog();
    const [, description] = Array.from(dialog().querySelectorAll<HTMLInputElement>("input"));
    await type(description, "");
    expect(button("Save template")!.disabled).toBe(true);
    expect(dialog().querySelector('[role="status"]')?.textContent).toContain("what it does");
  });

  it("saves the exact identity and opt-outs the owner chose", async () => {
    await renderDialog();
    const [, description] = Array.from(dialog().querySelectorAll<HTMLInputElement>("input"));
    await type(description, "Researches a market question and writes a cited report.");
    await act(async () => dialog().querySelector<HTMLButtonElement>('[data-payload-item="file:notes/brief.md"] [role="switch"]')!.click());
    await act(async () => button("Save template")!.click());
    const saved = distill.mock.calls[distill.mock.calls.length - 1][0];
    expect(saved).toMatchObject({
      team_id: "team_7", name: "me/research-desk", title: "Research desk", version: "1.0.0",
      description: "Researches a market question and writes a cited report.", opt_out: ["file:notes/brief.md"], save: true, format: "team",
    });
    expect(saved).not.toHaveProperty("dry_run");
    expect(onSaved).toHaveBeenCalledExactlyOnceWith(review.saved);
  });

  it("clears and restores a whole Includes group from its head link", async () => {
    await renderDialog();
    const includes = dialog().querySelector<HTMLElement>('[aria-label="Includes"]')!;
    await act(async () => includes.querySelector<HTMLButtonElement>('button[aria-label="Clear all includes"]')!.click());
    expect(Array.from(includes.querySelectorAll('[role="switch"]')).map((toggle) => toggle.getAttribute("aria-checked"))).toEqual(["false", "false"]);
    expect(includes.querySelector('[data-payload-item="skill:source-check"] [data-executable="true"]')).not.toBeNull();
    await act(async () => includes.querySelector<HTMLButtonElement>('button[aria-label="Select all includes"]')!.click());
    const [, description] = Array.from(dialog().querySelectorAll<HTMLInputElement>("input"));
    await type(description, "Researches a market question.");
    await act(async () => button("Save template")!.click());
    expect(distill.mock.calls[distill.mock.calls.length - 1][0]).toMatchObject({ opt_out: [], save: true });
  });

  it("opens Export with the saved template after Save and export", async () => {
    await renderDialog();
    const [, description] = Array.from(dialog().querySelectorAll<HTMLInputElement>("input"));
    await type(description, "Researches a market question.");
    await act(async () => button("Save and export…")!.click());
    expect(dialog().textContent).toContain("Export Research desk");
    expect(dialog().textContent).not.toContain("ada/research-desk");
    expect(dialog().querySelector('input[type="radio"][value="team"]')).toHaveProperty("checked", true);
  });
});
