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
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });

async function main() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let parentClicks = 0;

  await act(async () => {
    root.render(
      <div onClick={() => parentClicks++}>
        <DropdownMenu
          items={[{ label: "Rename", onClick: () => undefined }]}
          trigger={<button type="button">Actions</button>}
        />
      </div>,
    );
  });

  const trigger = host.querySelector("button") as unknown as { dispatchEvent(event: unknown): boolean } | null;
  if (!trigger) throw new Error("trigger should render");
  trigger.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
  if (parentClicks !== 0) throw new Error("dropdown trigger click must not select its parent row");

  await act(async () => root.unmount());
  console.log("DropdownMenu tests: 1/1 passed");
}

void main();
