import { sequence } from "../engine";
import { hold, plan, scene, shot } from "./library";

export default sequence("music", plan("music", [
  scene("opener-dots"),
  scene("team-glimpse"),
  scene("mesh-hero"),
  shot("mesh-reach"),
  shot("mesh-memory"),
  hold(scene("memory-graph"), 2),
  shot("team-return"),
  scene("plan-board"),
  scene("plugins"),
  scene("theme-repaint"),
  hold(shot("end-card"), 2),
]));
