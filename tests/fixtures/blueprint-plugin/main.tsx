import { createRoot } from "react-dom/client";
import "../../../src/styles/globals.css";
import { BlueprintView } from "../../../src/components/Automation/blueprint/BlueprintView";
import { document as definition, pluginNodes } from "../../../src/components/Automation/blueprint/pluginNodes.testdata";
import { pipelineModel } from "../../../src/components/Automation/pipelineModel";
import { blueprintNodeKey } from "../../../src/components/Automation/blueprintExpansion";
import { DEFAULT_THEME, themeToCSS } from "../../../src/lib/themeCss";

const params = new URLSearchParams(location.search);
const light = params.get("theme") === "light";
const theme = light ? { ...DEFAULT_THEME, background: "#ffffff", foreground: "#1f2328", black: "#24292f", white: "#f6f8fa", blue: "#0969da", magenta: "#8250df", cyan: "#1b7c83", green: "#1a7f37", yellow: "#9a6700", bright_black: "#656d76", bright_white: "#ffffff" } : DEFAULT_THEME;
for (const [key, value] of Object.entries(themeToCSS(theme))) window.document.documentElement.style.setProperty(key, value);
window.document.body.style.cssText = "margin:0;background:var(--ct-bg);color:var(--ct-fg);font-family:var(--ct-font-ui);";
window.document.getElementById("root")!.style.cssText = "height:100dvh;display:flex;flex-direction:column;padding:16px;box-sizing:border-box;";
const pipeline = pipelineModel({ id: "plugin-viewer-fixture", name: definition.name, state: "idle", run_id: null, current_step: null, last_event_at_ms: null, call_graph: definition, expanded_calls: [], plugin_nodes: pluginNodes });
createRoot(window.document.getElementById("root")!).render(<BlueprintView pipeline={pipeline} initialRoute={{ runId: null, trail: [], selected: params.get("details") === "no" ? null : blueprintNodeKey([], "read") }} history={false} fill />);
