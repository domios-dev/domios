import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { SidebarDrawer } from "./SidebarDrawer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("sidebar drawer wiring", () => {
  it.each([true, false])("keeps navigation available when shown (phone=%s)", async (phone) => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const onNavigate = vi.fn();
    try {
      await act(async () => root.render(
        <SidebarDrawer phone={phone} shown>
          <aside aria-label="Sidebar">
            <nav aria-label="Sidebar views"><button aria-label="Memory" onClick={onNavigate}>Memory</button></nav>
          </aside>
        </SidebarDrawer>,
      ));
      const navigation = host.querySelector('nav[aria-label="Sidebar views"]');
      expect(navigation).not.toBeNull();
      const button = navigation!.querySelector<HTMLButtonElement>('button[aria-label="Memory"]')!;
      expect(button).not.toBeNull();
      expect(button.closest('[aria-hidden="true"]')).toBeNull();
      await act(async () => button.click());
      expect(onNavigate).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("hides retained navigation from accessibility and interaction while closed", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(
        <SidebarDrawer phone shown={false}><aside aria-label="Sidebar" /></SidebarDrawer>,
      ));
      const sidebar = host.querySelector('aside[aria-label="Sidebar"]')!;
      expect(sidebar.closest('[aria-hidden="true"]')).not.toBeNull();
      expect(sidebar.closest("[inert]")).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
