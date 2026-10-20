import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";

const fake = vi.hoisted(() => ({
  update: {
    status: "available" as string,
    latestVersion: "1.14.9" as string | null,
    installing: false,
    progress: 0,
    applyUpdate: vi.fn(),
  },
}));

vi.mock("../hooks/useUpdate", () => ({ useUpdate: () => fake.update }));

import { UpdateBadge } from "./UpdateBadge";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let root: ReturnType<typeof createRoot> | null = null;
let host: HTMLDivElement | null = null;

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function renderBadge(update: Partial<typeof fake.update> = {}) {
  fake.update = {
    status: "available",
    latestVersion: "1.14.9",
    installing: false,
    progress: 0,
    applyUpdate: vi.fn(),
    ...update,
  };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<UpdateBadge />));
  return host;
}

it("uses the known version as the update button accessible name", async () => {
  const rendered = await renderBadge();

  expect(rendered.querySelector("button")?.getAttribute("aria-label")).toBe("Update 1.14.9");
});

it.each(["updates available", "updating"])("uses a status label instead of a version for %s", async (placeholder) => {
  const rendered = await renderBadge({ latestVersion: placeholder });
  const button = rendered.querySelector("button");

  expect(button?.getAttribute("aria-label")).toBe("Updates available");
  expect(button?.getAttribute("aria-label")).not.toBe("Update updates available");
  expect(button?.textContent).toContain("Updates available");
});

it("exposes an in-progress status without treating it as a version", async () => {
  const rendered = await renderBadge({ status: "downloading", latestVersion: "updating", installing: true, progress: 42 });

  expect(rendered.querySelector('[role="status"][aria-label="Updating"]')).not.toBeNull();
  expect(rendered.textContent).toContain("42%");
});
