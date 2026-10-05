// Run: node scripts/run-ts-unit-tests.mjs src/components/Extensions/PluginDetailPanel.layout.test.tsx

import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import type { CatalogEntry } from "./pluginCatalog";
import { PluginDetailPanel } from "./PluginDetailPanel";

const entry: CatalogEntry = {
  id: "telegram-client",
  displayName: "Telegram Client",
  installed: true,
  inMarketplace: true,
  system: false,
  verified: true,
  removable: true,
  dev: false,
  compatible: true,
  updateAvailable: false,
  version: "1.0.0",
  configHelp: "Choose an account.",
};

const markup = renderToStaticMarkup(<PluginDetailPanel
  entry={entry}
  config={null}
  busy={null}
  onAction={() => {}}
  onHostGrant={() => {}}
  onSave={async () => {}}
  onUpdateFullConfig={() => {}}
  onGone={() => {}}
  activityDisplayMap={new Map()}
/>);
assert.match(markup, /data-testid="extensions-plugin-header"[^>]*style="[^"]*display:flex;flex-wrap:wrap/);
assert.match(markup, /data-testid="extensions-plugin-name"[^>]*style="[^"]*white-space:nowrap;overflow:hidden;text-overflow:ellipsis/);
assert.match(markup, /data-testid="extensions-plugin-metadata"[^>]*style="[^"]*white-space:nowrap;overflow:hidden;text-overflow:ellipsis/);
assert.match(markup, /aria-label="Configure Telegram Client with AI"/);
assert.match(markup, /extensions-plugin-name"[^>]*>Telegram Client<\/div>/);
console.log("Plugin detail header keeps its title and metadata on one ellipsized row beside an accessible configure action");
