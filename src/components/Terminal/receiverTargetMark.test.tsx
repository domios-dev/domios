import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";
import {
  __setPluginTabMenuForTest,
  type PluginTabMenuHost,
  type PluginTabMenuState,
} from "../../lib/pluginTabMenu";
import { ReceiverTargetMarks } from "./ReceiverTargetMark";
import { usePluginTabMenu } from "../../lib/pluginTabMenu";

const SANITIZED_ICON = '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M2 2h20v20H2z"/></svg>';

function menuState(targets: Record<string, string>): PluginTabMenuState {
  return {
    items: [
      {
        pluginId: "real-receiver",
        itemId: "receiver-target",
        label: "Receive events here",
        action: "receiverTarget",
        iconSvg: SANITIZED_ICON,
        iconColor: "#d97757",
      },
      {
        pluginId: "future-receiver",
        itemId: "receiver-target",
        label: "Receive events here",
        action: "receiverTarget",
        iconSvg: SANITIZED_ICON,
        iconColor: "#d97757",
      },
    ],
    receiverTargets: targets,
  };
}

function renderSurface(menu: PluginTabMenuState, tabId = "tab-a") {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  act(() => {
    root.render(<ReceiverTargetMarks menu={menu} tabId={tabId} surface="composer" />);
  });
  return { host, root };
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

function cleanup(root: Root, host: HTMLElement): void {
  act(() => root.unmount());
  host.remove();
}

describe("receiver_target_mark", () => {
  it("receiver_target_mark renders real and future receiver capabilities with the same accessible surface", () => {
    const menu = menuState({ "real-receiver": "tab-a", "future-receiver": "tab-a" });
    menu.items.push({ ...menu.items[0], itemId: "receiver-target-secondary" });
    const { host, root } = renderSurface(menu);
    const marks = host.querySelectorAll("button[data-receiver-plugin-id]");
    expect(marks).toHaveLength(2);
    expect([...marks].map((mark) => mark.getAttribute("aria-label"))).toEqual([
      "Detach real-receiver receiver from this tab",
      "Detach future-receiver receiver from this tab",
    ]);
    expect(marks[0]?.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
    expect(marks[1]?.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
    expect(marks[0]?.querySelector("svg")?.getAttribute("width")).toBe("16");
    expect(marks[1]?.querySelector("svg")?.getAttribute("width")).toBe("16");
    expect((marks[0]?.querySelector("span") as HTMLElement | null)?.getAttribute("style")).toContain("color");
    expect((marks[1]?.querySelector("span") as HTMLElement | null)?.getAttribute("style")).toContain("color");
    cleanup(root, host);
  });

  it("receiver_target_mark uses the honest puzzle fallback when sanitized icon data is unavailable", () => {
    const menu = menuState({ "real-receiver": "tab-a" });
    menu.items[0] = { ...menu.items[0], iconSvg: null, iconColor: null };
    const { host, root } = renderSurface(menu);
    const mark = host.querySelector("button[data-receiver-plugin-id]");
    expect(mark?.querySelector("svg")).not.toBeNull();
    expect(mark?.querySelectorAll("span")).toHaveLength(1);
    expect(mark?.getAttribute("aria-label")).toBe("Detach real-receiver receiver from this tab");
    cleanup(root, host);
  });

  it("receiver_target_mark moves bound marks on contributions-changed without retaining a stale tab mark", async () => {
    let current = menuState({ "real-receiver": "tab-a" });
    let notify: (() => void) | null = null;
    const host: PluginTabMenuHost = {
      available: () => true,
      load: async () => current,
      run: async () => current,
      watch: async (onChange) => {
        notify = onChange;
        return () => {
          notify = null;
        };
      },
    };
    __setPluginTabMenuForTest(current, host);
    const rootHost = document.createElement("div");
    document.body.appendChild(rootHost);
    const root = createRoot(rootHost);
    function LiveSurfaces() {
      const menu = usePluginTabMenu();
      return (
        <>
          <ReceiverTargetMarks menu={menu} tabId="tab-a" surface="composer" />
          <ReceiverTargetMarks menu={menu} tabId="tab-b" surface="header" />
        </>
      );
    }
    await act(async () => root.render(<LiveSurfaces />));
    await flush();
    expect(rootHost.querySelector('[data-testid="receiver-target-marks-composer"]')).not.toBeNull();
    expect(rootHost.querySelector('[data-testid="receiver-target-marks-header"]')).toBeNull();

    current = menuState({ "real-receiver": "tab-b" });
    await act(async () => {
      notify?.();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(rootHost.querySelector('[data-testid="receiver-target-marks-composer"]')).toBeNull();
    expect(rootHost.querySelector('[data-testid="receiver-target-marks-header"]')).not.toBeNull();

    current = menuState({});
    await act(async () => {
      notify?.();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(rootHost.querySelector('[data-testid="receiver-target-marks-composer"]')).toBeNull();
    expect(rootHost.querySelector('[data-testid="receiver-target-marks-header"]')).toBeNull();
    cleanup(root, rootHost);
    __setPluginTabMenuForTest({ items: [], receiverTargets: {} }, host);
  });

  it("receiver_target_mark detaches through the host binding operation and remains keyboard reachable", async () => {
    const calls: Array<{ pluginId: string; itemId: string; tabId: string }> = [];
    const host: PluginTabMenuHost = {
      available: () => true,
      load: async () => menuState({ "real-receiver": "tab-a" }),
      run: async (item, tabId) => {
        calls.push({ pluginId: item.pluginId, itemId: item.itemId, tabId });
        return menuState({});
      },
      watch: async () => () => {},
    };
    __setPluginTabMenuForTest(menuState({ "real-receiver": "tab-a" }), host);
    const rootHost = document.createElement("div");
    document.body.appendChild(rootHost);
    const root = createRoot(rootHost);
    function LiveSurface() {
      const menu = usePluginTabMenu();
      return <ReceiverTargetMarks menu={menu} tabId="tab-a" surface="composer" />;
    }
    await act(async () => root.render(<LiveSurface />));
    await flush();
    const mark = rootHost.querySelector<HTMLButtonElement>("button[data-receiver-plugin-id]");
    expect(mark?.title).toBe("Detach real-receiver receiver from this tab");
    expect(mark?.disabled).toBe(false);
    await act(async () => {
      mark?.click();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(calls).toEqual([{ pluginId: "real-receiver", itemId: "receiver-target", tabId: "tab-a" }]);
    expect(rootHost.querySelector("button[data-receiver-plugin-id]")).toBeNull();
    cleanup(root, rootHost);
    __setPluginTabMenuForTest({ items: [], receiverTargets: {} }, host);
  });
});
