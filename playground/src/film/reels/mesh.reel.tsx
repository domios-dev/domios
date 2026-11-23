import { sequence } from "../engine";
import { plan, scene, shot } from "./library";

export default sequence("mesh", plan("mesh", [
  scene("opener-dots"),
  scene("team-glimpse"),
  scene("mesh-hero"),
  shot("mesh-reach"),
  shot("mesh-memory"),
  scene("memory-graph"),
  shot("team-return", 5.4),
  scene("theme-repaint"),
  shot("end-card"),
]));
