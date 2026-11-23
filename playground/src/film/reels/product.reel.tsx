import { sequence } from "../engine";
import { plan, scene, shot, trim } from "./library";

export default sequence("product", plan("product", [
  scene("opener-dots"),
  scene("team-glimpse"),
  trim(scene("mesh-hero"), 0, 6),
  shot("mesh-reach", 5.6),
  shot("mesh-memory", 5.4),
  trim(scene("memory-graph"), 0, 5),
  shot("team-return"),
  scene("jobs"),
  trim(scene("plan-board"), 0, 4),
  scene("plugins"),
  scene("browser"),
  shot("chat-automation"),
  scene("theme-repaint"),
  shot("end-card"),
]));
