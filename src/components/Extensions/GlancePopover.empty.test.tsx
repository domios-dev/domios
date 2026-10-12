import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";

async function main() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const [{ __setInvokeForTest }, { GlancePopover }, { __resetPluginSnapshotStoreForTest }] = await Promise.all([
    import("../../lib/bridge"),
    import("./GlancePopover"),
    import("../../lib/pluginSnapshotStore"),
  ]);
  let supportsGlance = false;
  let glanceRequests = 0;
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "plugin_manager_snapshot") return {
      plugins: [{ id: "provider-less", capabilities: { glance_view: supportsGlance } }],
    } as T;
    assert.equal(command, "plugin_glance");
    glanceRequests++;
    return null as T;
  });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    for (const scenario of [
      { capable: false, configHelp: undefined },
      { capable: false, configHelp: "fixture-configuration" },
      { capable: true, configHelp: undefined },
    ]) {
      const { capable, configHelp } = scenario;
      supportsGlance = capable;
      glanceRequests = 0;
      __resetPluginSnapshotStoreForTest();
      let settingsPlugin: string | undefined;
      let closes = 0;
      await act(async () => root.render(<GlancePopover
        key={`${capable}:${String(configHelp)}`}
        pluginId="provider-less"
        displayName="Fixture plugin"
        configHelp={configHelp}
        anchor={{ right: 0 }}
        onClose={() => { closes++; }}
        onOpenSettings={(id) => { settingsPlugin = id; }}
      />));
      assert.equal(glanceRequests, capable ? 1 : 0);
      const dialog = document.querySelector('[role="dialog"]');
      assert.ok(dialog);
      assert.equal(dialog.querySelector('[role="alert"]'), null);
      const status = dialog.querySelector('[role="status"]');
      if (configHelp) {
        assert.equal(status, null);
        assert.equal(dialog.querySelectorAll("button").length, 2);
      } else {
        assert.ok(status, "Absent glance resolves to a neutral status");
        assert.equal(status.getAttribute("aria-busy"), "false");
      }
      const settings = dialog.querySelector<HTMLButtonElement>('button[aria-label="Open settings"]');
      assert.ok(settings);
      await act(async () => settings.click());
      assert.equal(settingsPlugin, "provider-less");
      assert.equal(closes, 1);
    }
    console.log("PASS: 3 absent-glance cases resolve without errors and retain settings access");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
  }
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
