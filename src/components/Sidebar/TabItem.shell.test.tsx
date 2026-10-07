import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { renderToStaticMarkup } from "react-dom/server";
import { TabItem } from "./TabItem";
import { ShellStatus } from "../shared/ShellStatus";
import type { Tab } from "../../lib/types";
import type { ShellPurpose } from "../../lib/shellCommandState";

const document = new Window().document;
for (const purpose of ["build", "dev", "qa", "shell"] as ShellPurpose[]) {
  const tab: Tab = { id: "utility", pty_id: "pty", tab_type: "Terminal", title: "Utility", cwd: null, purpose };
  const label = purpose === "shell" ? "shell" : `${purpose} shell`;
  document.body.innerHTML = renderToStaticMarkup(<TabItem tab={tab} index={0} isActive={false}
    onClick={() => undefined} onClose={() => undefined} canClose workspaceId="workspace" />);
  assert.ok(document.querySelector(`button[aria-label="Open tab ${label}, Utility"]`));
  assert.ok(document.querySelector(`[role="img"][aria-label="${label}"]`));
  assert.ok(document.querySelector(`[role="status"][aria-label="${label}, Utility: unknown"]`));
  for (const [state, status] of [
    [{ activity: "running", last_exit_code: null }, "running"],
    [{ activity: "idle", last_exit_code: 0 }, "idle, last exit ok"],
    [{ activity: "idle", last_exit_code: 2 }, "idle, last exit failed (2)"],
  ] as const) {
    document.body.innerHTML = renderToStaticMarkup(<ShellStatus tab={{ ...tab, shell_command_state: state }} />);
    assert.ok(document.querySelector(`[role="status"][aria-label="${label}, Utility: ${status}"]`));
  }
}
console.log("TabItem shell wiring: passed");
