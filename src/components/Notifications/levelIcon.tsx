import { ErrorIcon, InfoIcon, WarningIcon } from "../Icons";
import type { NotifyLevelName } from "../../lib/notifyLevels";

const LEVEL_ICON: Record<NotifyLevelName, typeof InfoIcon> = {
  verbose: InfoIcon,
  action_needed: WarningIcon,
  failure: ErrorIcon,
};

/** An unlevelled record takes the neutral glyph, the same deliberate choice its
 *  tone makes — a record that claimed nothing is never drawn as a warning. */
export function levelIcon(level: NotifyLevelName | null): typeof InfoIcon {
  return level === null ? InfoIcon : LEVEL_ICON[level];
}
