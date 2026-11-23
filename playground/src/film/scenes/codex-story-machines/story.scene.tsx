import { sequence } from "../../engine";
import safe from "./safe-point.scene";
import confirm from "./confirm.scene";
import update from "./update.scene";
import restored from "./restored.scene";
import cooling from "./cooling.scene";
import owner from "./owner.scene";
export default sequence("cx-story-machines-story", [safe, confirm, update, restored, cooling, owner]);
