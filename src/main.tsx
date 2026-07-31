import ReactDOM from "react-dom/client";
import { App } from "./App";
import { FrontendCrashBoundary } from "./components/FrontendCrashBoundary";
import { WebGate } from "./components/WebGate/WebGate";
import { isMissionControlPrototypeRoute, MissionControlPrototype } from "./components/Canvas/missionControl";
import { loadBundledPlugins } from "./lib/plugins/loader";
import { installToolOutputCardHarness } from "./components/AgentRun/ToolOutputCard.browserHarness";
import { installInboundReportCardHarness } from "@codeterm/chat-ui";
import { installCodeTermTestChatHandle, isCodeTermFrontendTestMode } from "./components/Terminal/useChatSession";
import "./styles/globals.css";
import "./components/FrontendCrashBoundary.css";
import "./components/Canvas/missionControl/missionControl.css";

const root = document.getElementById("root")!;
const missionControlPrototype = isMissionControlPrototypeRoute();

const isGalleryRoute = new URLSearchParams(window.location.search).get("prototype") === "gallery";

installToolOutputCardHarness();
installInboundReportCardHarness(isCodeTermFrontendTestMode());
installCodeTermTestChatHandle();

if (!missionControlPrototype && !isGalleryRoute) {
  loadBundledPlugins();
}

if (isGalleryRoute) {
  import("./components/Gallery/ComponentGallery").then(({ ComponentGallery }) => {
    ReactDOM.createRoot(root).render(<ComponentGallery />);
  });
} else {
  ReactDOM.createRoot(root).render(
    <FrontendCrashBoundary>
      {missionControlPrototype ? (
        <MissionControlPrototype />
      ) : (
        <WebGate>
          <App />
        </WebGate>
      )}
    </FrontendCrashBoundary>,
  );
}

document.documentElement.dataset.codetermMounted = "true";
