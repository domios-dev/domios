import assert from "node:assert/strict";
import type { Tab } from "../../lib/types";
import { countWorkingTabs } from "./tabStatuses";

const tabs = [
  { id: "running", title: "Running", tab_type: "Terminal", pty_id: null, cwd: null, shell_command_state: { activity: "running", last_exit_code: null } },
  { id: "idle", title: "Idle", tab_type: "Terminal", pty_id: null, cwd: null, shell_command_state: { activity: "idle", last_exit_code: 0 } },
] satisfies Tab[];

assert.equal(countWorkingTabs(tabs, {}), 1);
assert.equal(countWorkingTabs(tabs.slice(1), {}), 0);
console.log("sidebar working tab selector: passed");
