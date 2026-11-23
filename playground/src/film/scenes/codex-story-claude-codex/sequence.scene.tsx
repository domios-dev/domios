import { sequence } from "../../engine";
import hook from "./01-hook.scene";
import lanes from "./02-lanes.scene";
import reports from "./03-reports.scene";
import review from "./04-review.scene";
import payoff from "./05-payoff.scene";
export default sequence("cx-story-claude-codex-story", [hook, lanes, reports, review, payoff]);
