import ReactDOM from "react-dom/client";
import { App } from "./App";
import { FrontendCrashBoundary } from "./components/FrontendCrashBoundary";
import { WebGate } from "./components/WebGate/WebGate";
import { isMissionControlPrototypeRoute, MissionControlPrototype } from "./components/Canvas/missionControl";
import { loadBundledPlugins } from "./lib/plugins/loader";
import { installToolOutputCardHarness } from "./components/AgentRun/ToolOutputCard.browserHarness";
import {
  configureHtmlFileLoader,
  configureLocalArtifactLoader,
  installInboundReportCardHarness,
} from "@codeterm/chat-ui";
import { invoke } from "./lib/bridge";
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
configureHtmlFileLoader((path) => invoke<string>("read_html_preview_file", { path }));
configureLocalArtifactLoader((path, kind) => invoke("read_local_preview_file", { path, kind }));

if (!missionControlPrototype && !isGalleryRoute) {
  loadBundledPlugins();
}

if (isGalleryRoute) {
  import("./components/Gallery/ComponentGallery").then(({ ComponentGallery }) => {
    ReactDOM.createRoot(root).render(<ComponentGallery />);
    document.documentElement.dataset.codetermMounted = "true";
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
  document.documentElement.dataset.codetermMounted = "true";
}
