import { sequence } from "../engine";
import { plan, scene, shot } from "./library";

export default sequence("playful", plan("playful", [
  scene("opener-dots"),
  scene("team-glimpse"),
  scene("mesh-hero"),
  shot("mesh-reach"),
  shot("mesh-memory"),
  scene("memory-graph"),
  scene("browser"),
  shot("chat-automation"),
  scene("plugins"),
  scene("jobs"),
  scene("theme-repaint"),
  shot("end-card"),
]));
