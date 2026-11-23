import { sequence } from "../engine";
import { plan, scene, shot } from "./library";

export default sequence("serious", plan("serious", [
  scene("opener-dots"),
  scene("team-glimpse"),
  scene("mesh-hero"),
  shot("mesh-reach"),
  shot("mesh-memory"),
  scene("memory-graph"),
  shot("team-return"),
  scene("plan-board"),
  scene("jobs"),
  shot("chat-automation"),
  scene("browser"),
  scene("plugins"),
  scene("theme-repaint"),
  shot("end-card"),
]));
