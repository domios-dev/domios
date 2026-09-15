import "../../../../src/styles/tokens.css";
import "../../../../src/styles/primitives.css";
import "../../../../src/components/Canvas/missionControl/missionControlBoard.css";
import "./analytics.css";
import { createRoot } from "react-dom/client";
import { Dashboard } from "./Dashboard";

const root = document.getElementById("ct-root");
if (root) createRoot(root).render(<Dashboard />);
