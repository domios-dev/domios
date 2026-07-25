// Run: npx tsx src/components/Extensions/pluginCatalog.test.ts

import {
  buildCatalog,
  canUninstall,
  filterCatalog,
  primaryAction,
  searchMatches,
  updateCount,
} from "./pluginCatalog";
import type { PluginSnapshot } from "./types";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

const snapshot: PluginSnapshot = {
  plugins: [
    { id: "claude", display_name: "Claude", system: true, removable: false, version: "1.4.6", capabilities: { agent_provider: true } },
    { id: "git", display_name: "Git", system: false, removable: true, version: "0.1.0", capabilities: { status_bubble: true }, description: "Shows branch status." },
    { id: "bitwarden", display_name: "Bitwarden", system: false, removable: true, version: "0.1.0", capabilities: { secret_backend: true }, config_help: "Choose a vault server." },
  ],
  channels: [],
  diff: [],
  marketplace: [
    // Installed + newer in channel → update available.
    { channel: "core", id: "git", local_version: "0.1.0", remote_version: "0.2.0", drift: "update_available", compatible: true, installed: true },
    // Not installed, compatible → installable.
    { channel: "store", id: "transcriber", remote_version: "0.1.1", drift: "not_installed", compatible: true, installed: false, display_name: "Transcriber", description: "Speech to text.", capabilities: { transcriber: true } },
    // Not installed, incompatible → blocked.
    { channel: "store", id: "future", remote_version: "9.0.0", drift: "not_installed", compatible: false, installed: false, blocked_reason: "requires CodeTerm ≥ 9.0.0" },
  ],
};

test("buildCatalog merges installed + marketplace by id", () => {
  const cat = buildCatalog(snapshot);
  const ids = cat.map((e) => e.id).sort();
  assert(ids.join(",") === "bitwarden,claude,future,git,transcriber", `got ${ids.join(",")}`);
});

test("pending_reload flows from snapshot into catalog entry (§1.8)", () => {
  const cat = buildCatalog({
    plugins: [
      { id: "needsreload", display_name: "Needs Reload", system: false, removable: true, version: "1.0.0", capabilities: { status_bubble: true }, requires_reload: true, pending_reload: true },
    ],
    channels: [],
    diff: [],
  });
  const e = cat.find((x) => x.id === "needsreload")!;
  assert(e.pendingReload === true, "pendingReload must propagate to the catalog entry");
});

test("installed-only plugin (bitwarden) survives even without a channel entry", () => {
  const cat = buildCatalog(snapshot);
  const bw = cat.find((e) => e.id === "bitwarden");
  assert(bw?.installed === true && bw.inMarketplace === false, "bitwarden must be installed, not in marketplace");
  assert(bw.configHelp === "Choose a vault server.", "explicit configHelp must propagate");
  assert(buildCatalog(snapshot).find((e) => e.id === "git")?.configHelp == null, "description-only plugins must not become AI-configurable");
});

test("installed plugin in channel marks update + keeps installed metadata", () => {
  const git = buildCatalog(snapshot).find((e) => e.id === "git")!;
  assert(git.installed && git.inMarketplace, "git is both");
  assert(git.updateAvailable === true, "git update available");
  assert(git.version === "0.1.0" && git.remoteVersion === "0.2.0", "versions");
  assert(git.description === "Shows branch status.", "installed description wins");
});

test("not-installed channel plugin pulls channel metadata", () => {
  const t = buildCatalog(snapshot).find((e) => e.id === "transcriber")!;
  assert(!t.installed && t.inMarketplace, "transcriber from channel only");
  assert(t.displayName === "Transcriber" && t.description === "Speech to text.", "channel metadata");
});

test("primaryAction: not installed compatible → Install", () => {
  const t = buildCatalog(snapshot).find((e) => e.id === "transcriber")!;
  const a = primaryAction(t);
  assert(a.kind === "install" && !a.disabled, "installable");
});

test("primaryAction: incompatible → disabled Install with reason", () => {
  const f = buildCatalog(snapshot).find((e) => e.id === "future")!;
  const a = primaryAction(f);
  assert(a.kind === "install" && a.disabled, "blocked install");
  assert(!!a.reason && a.reason.includes("9.0.0"), "reason carried");
});

test("primaryAction: installed + update → Update", () => {
  const git = buildCatalog(snapshot).find((e) => e.id === "git")!;
  assert(primaryAction(git).kind === "update", "update primary");
});

test("primaryAction: installed removable, no update → Uninstall", () => {
  const bw = buildCatalog(snapshot).find((e) => e.id === "bitwarden")!;
  assert(primaryAction(bw).kind === "uninstall", "uninstall primary");
});

test("primaryAction: system plugin → none/disabled, never uninstall", () => {
  const claude = buildCatalog(snapshot).find((e) => e.id === "claude")!;
  const a = primaryAction(claude);
  assert(a.kind === "none" && a.disabled, "system not actionable");
  assert(canUninstall(claude) === false, "system never uninstallable");
});

test("canUninstall true for removable installed, even when update available", () => {
  const git = buildCatalog(snapshot).find((e) => e.id === "git")!;
  assert(canUninstall(git) === true, "git uninstallable alongside update");
});

test("filter: installed only", () => {
  const cat = buildCatalog(snapshot);
  const ids = filterCatalog(cat, "", "installed").map((e) => e.id).sort();
  assert(ids.join(",") === "bitwarden,claude,git", `got ${ids.join(",")}`);
});

test("filter: marketplace shows channel-present (incl. installed git)", () => {
  const cat = buildCatalog(snapshot);
  const ids = filterCatalog(cat, "", "marketplace").map((e) => e.id).sort();
  assert(ids.join(",") === "future,git,transcriber", `got ${ids.join(",")}`);
});

test("filter: updates only", () => {
  const cat = buildCatalog(snapshot);
  const ids = filterCatalog(cat, "", "updates").map((e) => e.id);
  assert(ids.join(",") === "git", `got ${ids.join(",")}`);
  assert(updateCount(cat) === 1, "one update");
});

test("optional marketplace update drift is surfaced as update", () => {
  const withOptionalUpdate = buildCatalog({
    ...snapshot,
    marketplace: [
      ...snapshot.marketplace,
      {
        channel: "store",
        id: "optional-plugin",
        local_version: "1.0.0",
        remote_version: "1.1.0",
        drift: "optional_update",
        compatible: true,
        installed: true,
      },
    ],
  });
  const optional = withOptionalUpdate.find((e) => e.id === "optional-plugin")!;
  assert(optional.installed, "optional-plugin must be installed");
  assert(optional.updateAvailable, "optional update drift must be exposed");
  assert(primaryAction(optional).kind === "update", "optional update should use Update primary action");
  const updates = filterCatalog(withOptionalUpdate, "", "updates").map((e) => e.id);
  assert(updates.includes("optional-plugin"), "optional update should appear in Updates filter");
  assert(updateCount(withOptionalUpdate) === 2, "two updates should be counted");
});

test("search matches id, name, capability, description, channel", () => {
  const cat = buildCatalog(snapshot);
  assert(filterCatalog(cat, "speech", "all").map((e) => e.id).join(",") === "transcriber", "description match");
  assert(filterCatalog(cat, "secret_backend", "all").map((e) => e.id).join(",") === "bitwarden", "capability match");
  assert(filterCatalog(cat, "store", "all").map((e) => e.id).sort().join(",") === "future,transcriber", "channel match");
  const git = cat.find((e) => e.id === "git")!;
  assert(searchMatches(git, "") === true, "empty query matches all");
});

test("filter sort: installed before marketplace-only, then alpha", () => {
  const cat = buildCatalog(snapshot);
  const order = filterCatalog(cat, "", "all").map((e) => e.id);
  assert(order.join(",") === "bitwarden,claude,git,future,transcriber", `got ${order.join(",")}`);
});

test("dev (workspace) plugin: flagged, not actionable, never uninstall", () => {
  const cat = buildCatalog({
    plugins: [{ id: "_viewtest", display_name: "View Test", system: false, removable: false, dev: true, version: "0.1.0", capabilities: { view: true } }],
    channels: [],
    diff: [],
    marketplace: [],
  });
  const vt = cat.find((e) => e.id === "_viewtest")!;
  assert(vt.dev === true && vt.installed === true, "dev plugin is loaded/installed but flagged dev");
  const a = primaryAction(vt);
  assert(a.kind === "none" && a.disabled && a.label === "Dev plugin", `expected Dev plugin none-action, got ${a.kind}/${a.label}`);
  assert(!!a.reason && a.reason.includes("workspace"), "reason explains workspace origin");
  assert(canUninstall(vt) === false, "dev plugin must not offer uninstall (remove would fail)");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(err);
  }
}
console.log(`pluginCatalog: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
