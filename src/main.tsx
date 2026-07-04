import ReactDOM from "react-dom/client";
import { App } from "./App";
import { WebGate } from "./components/WebGate/WebGate";
import { isMissionControlPrototypeRoute, MissionControlPrototype } from "./components/Canvas/missionControl";
import { loadBundledPlugins } from "./lib/plugins/loader";
import "./styles/globals.css";
import "./components/Canvas/missionControl/missionControl.css";

const root = document.getElementById("root")!;
const missionControlPrototype = isMissionControlPrototypeRoute();

// Dev-only: ?prototype=gallery renders the shared-primitive QA matrix without the full app.
const isGalleryRoute = import.meta.env.DEV && new URLSearchParams(window.location.search).get("prototype") === "gallery";

if (!missionControlPrototype && !isGalleryRoute) {
  // Plugins must be registered before any pane renders so the ChatView host can synchronously look up the right renderer for an agent_type.
  loadBundledPlugins();
}

if (isGalleryRoute) {
  // Lazy import keeps gallery code out of the production bundle entirely.
  import("./components/Gallery/ComponentGallery").then(({ ComponentGallery }) => {
    ReactDOM.createRoot(root).render(<ComponentGallery />);
  });
} else {
  ReactDOM.createRoot(root).render(
    missionControlPrototype ? (
      <MissionControlPrototype />
    ) : (
      <WebGate>
        <App />
      </WebGate>
    ),
  );
}
