import { installRequestDiagnostics } from "./lib/requestDiagnostics";
import { frontendBootDiagnostic } from "./lib/frontendBootDiagnostic";
import { DaemonFrontendBootstrap } from "./components/DaemonFrontendBootstrap";
import { ensureDaemonRuntime } from "./lib/daemonRuntime";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { FrontendCrashBoundary } from "./components/FrontendCrashBoundary";
import { WebGate } from "./components/WebGate/WebGate";
import { frontendPrototypeRoute } from "./lib/frontendPrototypeRoute";
import { deferredComponent } from "./components/shared/deferredComponent";
import { loadBundledPlugins } from "./lib/plugins/loader";
import { installToolOutputCardHarness } from "./components/AgentRun/ToolOutputCard.browserHarness";
import { installAgentStateIndicatorHarness } from "./components/Terminal/AgentStateIndicator.browserHarness";
import {
  installInboundReportCardHarness,
} from "@codeterm/chat-ui";
import { configureLocalPreviews } from "./lib/configureLocalPreviews";
import { installScrollActivity } from "./lib/scrollActivity";
import { installCodeTermTestChatHandle, isCodeTermFrontendTestMode } from "./components/Terminal/useChatSession";
// Stylesheets load here, never from a component: the plain-Node unit runner
// has no bundler, so a CSS import anywhere in a tested graph is a syntax error.
import "./styles/globals.css";
import "./components/FrontendCrashBoundary.css";
import "./components/Canvas/missionControl/missionControl.css";
import "./components/Canvas/missionControl/missionControlBoard.css";
import "./components/Usage/styles.css";
import "./components/Snake/SnakeGame.css";

const MissionControlPrototype = deferredComponent(async () => ({ default: (await import("./components/Canvas/missionControl/MissionControlPrototype")).MissionControlPrototype }));
const SnakeGame = deferredComponent(async () => ({ default: (await import("./components/Snake/SnakeGame")).SnakeGame }));

installRequestDiagnostics();
frontendBootDiagnostic("module-entry");
ensureDaemonRuntime();

const root = document.getElementById("root")!;
const prototype = frontendPrototypeRoute(window.location.search);
const missionControlPrototype = prototype === "mission-control";

const isGalleryRoute = prototype === "gallery";
const snakePrototype = prototype === "snake";

installToolOutputCardHarness();
installAgentStateIndicatorHarness();
installInboundReportCardHarness(isCodeTermFrontendTestMode());
installCodeTermTestChatHandle();
configureLocalPreviews();
installScrollActivity();

if (!missionControlPrototype && !isGalleryRoute && !snakePrototype) {
  loadBundledPlugins();
}

if (isGalleryRoute) {
  import("./components/Gallery/ComponentGallery").then(({ ComponentGallery }) => {
    ReactDOM.createRoot(root).render(<ComponentGallery />);
    document.documentElement.dataset.codetermMounted = "true";
  });
} else if (snakePrototype) {
  ReactDOM.createRoot(root).render(<SnakeGame />);
  document.documentElement.dataset.codetermMounted = "true";
} else {
  ReactDOM.createRoot(root).render(
    <FrontendCrashBoundary>
      {missionControlPrototype ? (
        <MissionControlPrototype />
      ) : (
        <WebGate>
          <DaemonFrontendBootstrap><App /></DaemonFrontendBootstrap>
        </WebGate>
      )}
    </FrontendCrashBoundary>,
  );
  document.documentElement.dataset.codetermMounted = "true";
}
