// `level` is optional, so a record carrying none is a legal state, not a lost
// one. It paints the neutral tone by name — the choice is `UNLEVELLED_TONE`,
import type { Notification } from "../../lib/types";
import { UNLEVELLED_TONE } from "../../lib/notifyLevels";
import {
  cardLevel,
  cardTone,
  panelLevel,
  panelTone,
  record,
  toastLevel,
  toastTone,
} from "./toneAxisSurfaces";

let checked = 0;
function assert(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

const SEVERITIES: Array<Notification["severity"]> = ["Info", "Warning", "Error"];

for (const severity of SEVERITIES) {
  const notification = record({ severity });
  assert(
    notification.level === undefined,
    "the unlevelled fixture must actually carry no level",
  );
  const panel = panelTone(notification);
  const card = cardTone(notification);
  const toast = toastTone(notification);
  assert(
    panel === card && panel === toast,
    `an unlevelled record must read the same everywhere: panel ${panel}, chat card ${card}, toast ${toast} (severity ${severity})`,
  );
  assert(
    `var(${panel})` === UNLEVELLED_TONE,
    `an unlevelled record must paint the declared neutral tone ${UNLEVELLED_TONE}, got var(${panel}) (severity ${severity})`,
  );
  for (const [surface, level] of [
    ["panel", panelLevel(notification)],
    ["chat card", cardLevel(notification)],
    ["toast", toastLevel(notification)],
  ] as const) {
    assert(
      level === "",
      `the ${surface} must keep an unlevelled record unlevelled, got "${level}" (severity ${severity})`,
    );
  }
}

console.log(`tone_axis_unlevelled tests: ${checked} assertions passed`);
