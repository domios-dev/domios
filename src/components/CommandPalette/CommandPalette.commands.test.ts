// Run: npx tsx src/components/CommandPalette/CommandPalette.commands.test.ts

import { buildDefaultCommands, filterCommands } from "./CommandPalette";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const calls: string[] = [];
const record = (name: string) => () => { calls.push(name); };
const actions = {
  newTab: record("newTab"),
  closeTab: record("closeTab"),
  newWorkspace: record("newWorkspace"),
  openSettings: record("openSettings"),
  openThemeSettings: record("openThemeSettings"),
  toggleSidebar: record("toggleSidebar"),
  spawnClaude: record("spawnClaude"),
  openNotifications: record("openNotifications"),
};
const tabs = [
  { workspaceId: "ws-a", workspaceName: "Alpha", tabId: "tab-1", title: "Build shell" },
  { workspaceId: "ws-b", workspaceName: "Beta", tabId: "tab-2", title: "Reviewer" },
];
const commands = buildDefaultCommands(actions, {
  bindings: { new_tab: "Ctrl+Alt+T" },
  tabs,
  goToTab: (workspaceId, tabId) => calls.push(`goToTab:${workspaceId}/${tabId}`),
});

// Every listed command does something, and no two commands do the same thing.
const effects = new Set<string>();
for (const command of commands) {
  calls.length = 0;
  command.action();
  assert(calls.length === 1, `${command.label} runs exactly one action (ran ${calls.length})`);
  assert(!effects.has(calls[0]), `${command.label} does not repeat another command's action ${calls[0]}`);
  effects.add(calls[0]);
}
assert(!commands.some((command) => command.id === "agent-hierarchy"), "no command opens a surface that does not exist");

const theme = commands.find((command) => command.id === "theme-switcher");
calls.length = 0;
theme?.action();
assert(calls[0] === "openThemeSettings", "Theme Switcher opens the theme settings, not the Settings landing page");

const withoutBell = buildDefaultCommands({ ...actions, openNotifications: undefined });
assert(!withoutBell.some((command) => command.id === "notifications"), "Notifications is absent while the bell is hidden");

assert(commands.find((command) => command.id === "new-tab")?.shortcut === "Ctrl+Alt+T", "shortcut labels follow the configured binding");
assert(commands.find((command) => command.id === "close-tab")?.shortcut === "Ctrl+W", "an unset binding shows the dispatched default");

const byWorkspace = filterCommands(commands, "beta");
assert(byWorkspace.length === 1 && byWorkspace[0].id === "go-to-tab:tab-2", "a Tab is found by its workspace name");
assert(filterCommands(commands, "reviewer")[0]?.id === "go-to-tab:tab-2", "a Tab is found by its title");
assert(filterCommands(commands, "zzz-no-such-command").length === 0, "an unmatched query has no results");

const interleaved = filterCommands([
  { id: "a1", label: "A one", section: "A", action: () => {} },
  { id: "b1", label: "B one", section: "B", action: () => {} },
  { id: "a2", label: "A two", section: "A", action: () => {} },
], "");
assert(interleaved.map((command) => command.id).join(",") === "a1,a2,b1", "results come back in the grouped order the list renders");

console.log("CommandPalette command contracts passed");
