import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getByRole } from "../../test/semanticRoles";

vi.mock("../Terminal/voice/useVoiceRecorder", () => ({
  useVoiceRecorder: () => ({ recording: false, preview: null, livePeaks: [] }),
}));

import { CartographerMapDialog } from "./CartographerMapDialog";
import { defaultCartographerMapDraft } from "./cartographerMapModel";
import { CARTOGRAPHER_TEST_PROVIDER } from "./cartographerMapFixtures.testdata";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const mounted: Root[] = [];
afterEach(async () => {
  await act(async () => { for (const root of mounted) root.unmount(); });
  mounted.length = 0;
  document.body.replaceChildren();
});

async function mount(element: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push(root);
  await act(async () => root.render(element));
}

describe("Cartographer map dialog wiring", () => {
  it("Cancel closes through its callback", async () => {
    const onCancel = vi.fn();
    await mount(<CartographerMapDialog mode="create" providers={[CARTOGRAPHER_TEST_PROVIDER]} onCancel={onCancel} onStart={vi.fn(async () => {})} />);
    await act(async () => getByRole(document.body, "button", "Cancel").click());
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  for (const mode of ["create", "refresh"] as const) {
    it(`Start calls the action once with the ${mode} model payload`, async () => {
      let resolve: () => void = () => {};
      const pending = new Promise<void>((done) => { resolve = done; });
      const onStart = vi.fn(() => pending);
      const onCancel = vi.fn();
      await mount(<CartographerMapDialog mode={mode} providers={[CARTOGRAPHER_TEST_PROVIDER]} onCancel={onCancel} onStart={onStart} />);
      const start = getByRole(document.body, "button", "Start");
      await act(async () => { start.click(); start.click(); });
      expect(onStart).toHaveBeenCalledTimes(1);
      expect(onStart).toHaveBeenCalledWith(defaultCartographerMapDraft(mode, [CARTOGRAPHER_TEST_PROVIDER]));
      expect(onCancel).not.toHaveBeenCalled();
      await act(async () => resolve());
      expect(onCancel).toHaveBeenCalledTimes(1);
    });
  }

  it("keeps a failed provider catalogue as a quiet unavailable state", async () => {
    await mount(<CartographerMapDialog mode="create" providers={[]} catalogueError="opaque catalogue failure" onCancel={vi.fn()} onStart={vi.fn(async () => {})} />);
    expect(document.body.querySelector('[role="status"]')).not.toBeNull();
    expect(document.body.querySelector('[role="alert"]')).toBeNull();
    expect((getByRole(document.body, "button", "Start") as HTMLButtonElement).disabled).toBe(true);
  });

  it("keeps a cached provider usable after a background catalogue failure", async () => {
    const onStart = vi.fn(async () => {});
    await mount(<CartographerMapDialog mode="create" providers={[CARTOGRAPHER_TEST_PROVIDER]} catalogueError="opaque catalogue failure" onCancel={vi.fn()} onStart={onStart} />);
    const start = getByRole(document.body, "button", "Start") as HTMLButtonElement;
    expect(document.body.querySelector('[role="alert"]')).toBeNull();
    expect(start.disabled).toBe(false);
    await act(async () => start.click());
    expect(onStart).toHaveBeenCalledTimes(1);
  });

  it("announces a failed user Start with one accessible alert", async () => {
    const onStart = vi.fn(async () => { throw new Error("opaque start failure"); });
    await mount(<CartographerMapDialog mode="create" providers={[CARTOGRAPHER_TEST_PROVIDER]} onCancel={vi.fn()} onStart={onStart} />);
    await act(async () => { getByRole(document.body, "button", "Start").click(); await Promise.resolve(); });
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(document.body.querySelectorAll('[role="alert"]')).toHaveLength(1);
  });
});
