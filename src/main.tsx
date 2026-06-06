import ReactDOM from "react-dom/client";
import { App } from "./App";
import { loadBundledPlugins } from "./lib/plugins/loader";
import "./styles/globals.css";

// Plugins must be registered before any pane renders so the ChatView
// host can synchronously look up the right renderer for an agent_type.
loadBundledPlugins();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <App />,
);
