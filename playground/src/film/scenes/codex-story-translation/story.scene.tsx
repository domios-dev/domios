import { sequence } from "../../engine";
import hook from "./hook.scene";
import lanes from "./lanes.scene";
import logic from "./logic.scene";
import fonts from "./fonts.scene";
import release from "./release.scene";

export default sequence("cx-story-translation-story", [hook, lanes, logic, fonts, release]);
