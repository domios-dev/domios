import { sequence } from "../engine";
import { plan, scene, shot, trim } from "./library";

export default sequence("fast", plan("fast", [
  scene("opener-dots"),
  scene("team-glimpse"),
  trim(scene("mesh-hero"), 0, 6),
  shot("mesh-reach", 5),
  shot("mesh-memory", 5),
  trim(scene("memory-graph"), 0, 5),
  shot("chat-automation", 4.8),
  trim(scene("jobs"), 0, 3.5),
  scene("theme-repaint"),
  shot("end-card"),
]));
