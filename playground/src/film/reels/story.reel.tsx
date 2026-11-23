import { sequence } from "../engine";
import { plan, scene, shot, trim } from "./library";

export default sequence("story", plan("story", [
  scene("opener-dots"),
  trim(scene("memory-graph"), 0, 6),
  scene("mesh-hero"),
  shot("mesh-reach"),
  shot("mesh-memory"),
  scene("team-glimpse"),
  shot("team-return"),
  scene("plan-board"),
  scene("jobs"),
  scene("plugins"),
  scene("theme-repaint"),
  shot("end-card"),
]));
