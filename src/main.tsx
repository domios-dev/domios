import ReactDOM from "react-dom/client";
import { App } from "./App";
import { WebGate } from "./components/WebGate/WebGate";
import { isMissionControlPrototypeRoute, MissionControlPrototype } from "./components/Canvas/missionControl";
import { loadBundledPlugins } from "./lib/plugins/loader";
import "./styles/globals.css";
import "./components/Canvas/missionControl/missionControl.css";

const root = document.getElementById("root")!;
const missionControlPrototype = isMissionControlPrototypeRoute();

if (!missionControlPrototype) {
  // Plugins must be registered before any pane renders so the ChatView host can synchronously look up the right renderer for an agent_type.
  loadBundledPlugins();
}

ReactDOM.createRoot(root).render(
  missionControlPrototype ? (
    <MissionControlPrototype />
  ) : (
    <WebGate>
      <App />
    </WebGate>
  ),
);
