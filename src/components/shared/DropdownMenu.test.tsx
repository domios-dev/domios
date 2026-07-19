import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { DropdownMenu } from "./DropdownMenu";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
(globalThis as { requestAnimationFrame: (callback: FrameRequestCallback) => number }).requestAnimationFrame = (callback) => {
  callback(0);
  return 0;
};
(globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};

async function main() {
  const styles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  const portalLayer = Number(styles.match(/\.ct-portal-menu\s*\{[^}]*z-index:\s*(\d+)/s)?.[1]);
  const modalLayer = Number(styles.match(/\.new-session-picker\s*\{[^}]*z-index:\s*(\d+)/s)?.[1]);
  if (!(portalLayer > modalLayer)) throw new Error("portal menu layer should stay above the new-session modal");

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let parentClicks = 0;
  let spawned = "";

  await act(async () => {
    root.render(
      <div onClick={() => parentClicks++}>
        <DropdownMenu
          items={[
            { label: "Rename", onClick: () => undefined },
            {
              label: "Quick spawn",
              submenu: {
                label: "Quick spawn",
                description: "Spawns a default Worker",
                items: [
                  { label: "Codex", onClick: () => { spawned = "codex"; } },
                  { label: "Claude Code", onClick: () => { spawned = "claude"; } },
                ],
              },
            },
          ]}
          trigger={<button type="button">Actions</button>}
        />
        <DropdownMenu
          items={[{ label: "Copy", onClick: () => undefined }]}
          trigger={<button type="button">More actions</button>}
        />
      </div>,
    );
  });

  const trigger = host.querySelector("button") as unknown as { dispatchEvent(event: unknown): boolean } | null;
  if (!trigger) throw new Error("trigger should render");
  trigger.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
  if (parentClicks !== 0) throw new Error("dropdown trigger click must not select its parent row");

  await act(async () => {
    trigger.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true }));
  });
  const rootMenu = document.querySelector('[role="menu"]');
  if (!rootMenu) throw new Error("pointer trigger should open an accessible menu");
  if (!rootMenu.classList.contains("ct-portal-menu")) throw new Error("portal menu should render above modal surfaces");
  const quickSpawn = Array.from(rootMenu.querySelectorAll('button[role="menuitem"]'))
    .find((button) => button.textContent?.includes("Quick spawn")) as HTMLButtonElement | undefined;
  if (quickSpawn?.getAttribute("aria-haspopup") !== "menu") throw new Error("submenu trigger should expose menu semantics");
  await act(async () => quickSpawn.click());
  const submenu = document.querySelector('[role="menu"]');
  if (!submenu?.textContent?.includes("Spawns a default Worker")) throw new Error("submenu should explain the default Worker behavior");
  if (submenu.textContent.includes("Rename")) throw new Error("submenu should replace root actions in the same popup");
  const codex = Array.from(submenu.querySelectorAll('button[role="menuitem"]'))
    .find((button) => button.textContent?.includes("Codex")) as HTMLButtonElement | undefined;
  await act(async () => codex?.click());
  if (spawned !== "codex") throw new Error("submenu action should invoke its spawn callback");
  if (document.querySelector('[role="menu"]')) throw new Error("submenu action should close the popup");

  await act(async () => {
    (trigger as unknown as HTMLButtonElement).focus();
    (trigger as unknown as HTMLButtonElement).dispatchEvent(new window.KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
    }) as unknown as Event);
  });
  if (!document.querySelector('[role="menu"]')) throw new Error("keyboard activation should open the menu");
  await act(async () => {
    document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }) as unknown as Event);
  });
  if (document.querySelector('[role="menu"]')) throw new Error("Escape should close the menu");

  await act(async () => {
    trigger.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true }));
  });
  const secondTrigger = Array.from(host.querySelectorAll("button"))
    .find((button) => button.textContent === "More actions");
  if (!secondTrigger) throw new Error("second dropdown trigger should render");
  await act(async () => {
    secondTrigger.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true }) as unknown as Event);
  });
  const openMenus = document.querySelectorAll('[role="menu"]');
  if (openMenus.length !== 1 || !openMenus[0]?.textContent?.includes("Copy")) {
    throw new Error("opening a dropdown should close the previously open dropdown");
  }

  await act(async () => root.unmount());
  console.log("DropdownMenu tests: 5/5 passed");
}

void main();
