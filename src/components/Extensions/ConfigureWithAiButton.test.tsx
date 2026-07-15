import { renderToStaticMarkup } from "react-dom/server";
import type { CatalogEntry } from "./pluginCatalog";
import { ConfigureWithAiButton } from "./ConfigureWithAiButton";

function entry(configHelp?: string): CatalogEntry {
  return {
    id: "demo",
    displayName: "Demo",
    installed: true,
    inMarketplace: true,
    system: false,
    verified: true,
    removable: true,
    dev: false,
    compatible: true,
    updateAvailable: false,
    configHelp,
  };
}

const optedIn = renderToStaticMarkup(<ConfigureWithAiButton entry={entry("Choose a server.")} />);
if (!optedIn.includes("Configure with AI")) throw new Error("explicit configHelp should enable AI configuration");

const zeroConfig = renderToStaticMarkup(<ConfigureWithAiButton entry={entry()} />);
if (zeroConfig !== "") throw new Error("plugins without configHelp should not render AI configuration");

console.log("ConfigureWithAiButton tests: 2/2 passed");
