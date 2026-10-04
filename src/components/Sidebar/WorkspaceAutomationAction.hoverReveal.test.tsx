import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { WorkspaceAutomationAction } from "../Automation/WorkspaceAutomationAction";

async function main() {
  const win = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event", "MouseEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const opened: string[] = [];
  let parentClicks = 0;
  const render = async (touchMode: boolean, revealOnHover: boolean) => {
    await act(async () => root.render(
      <div className="group" onClick={() => { parentClicks++; }}>
        <WorkspaceAutomationAction
          workspaceId="workspace-1"
          workspaceName="Project"
          onOpen={(workspaceId) => opened.push(workspaceId)}
          touchMode={touchMode}
          revealOnHover={revealOnHover}
        />
      </div>,
    ));
  };

  try {
    await render(false, true);
    const button = host.querySelector<HTMLButtonElement>('[data-testid="workspace-automation-action"]');
    assert.ok(button);
    assert.equal(button.type, "button");
    assert.equal(button.getAttribute("aria-label"), "Open jobs for Project");
    assert.equal(button.tabIndex, 0);
    for (const className of ["opacity-0", "group-hover:opacity-60", "focus-visible:opacity-100", "hover:!opacity-100"]) {
      assert.ok(button.classList.contains(className), `desktop action includes ${className}`);
    }
    button.focus();
    assert.equal(document.activeElement, button);
    await act(async () => button.click());
    assert.deepEqual(opened, ["workspace-1"]);
    assert.equal(parentClicks, 0);

    await render(true, true);
    const touchButton = host.querySelector<HTMLButtonElement>('[data-testid="workspace-automation-action"]');
    assert.ok(touchButton);
    assert.ok(touchButton.classList.contains("h-11"));
    assert.ok(touchButton.classList.contains("w-11"));
    assert.ok(!touchButton.classList.contains("opacity-0"));

    await render(false, false);
    const defaultButton = host.querySelector<HTMLButtonElement>('[data-testid="workspace-automation-action"]');
    assert.ok(defaultButton);
    assert.ok(!defaultButton.classList.contains("opacity-0"));
    console.log("workspace automation action preserves its behavior and reveals on hover, focus, and touch");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
