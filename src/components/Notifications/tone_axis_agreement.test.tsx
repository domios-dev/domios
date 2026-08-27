// Run: npm run test:unit -- tone_axis
//
// The defect this suite exists for: one record, two tone vocabularies. The bell
// panel painted from `severity`, the chat card from `level`, so a single record
// could read calm on one surface and urgent on the other. Both surfaces are
// driven here from ONE fixture and must agree.
import type { Notification } from "../../lib/types";
import { cardTone, panelTone, record, toastTone } from "./toneAxisSurfaces";

let checked = 0;
function assert(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

type Level = NonNullable<Notification["level"]>;

// Records whose two axes disagree on purpose — the exact shape that used to
// render two different tones for one notification.
const CROSSED: Array<[Notification["severity"], Level, string]> = [
  ["Info", "failure", "--ct-red"],
  ["Error", "verbose", "--ct-muted-fg"],
  ["Info", "action_needed", "--ct-yellow"],
  ["Warning", "failure", "--ct-red"],
];

for (const [severity, level, expected] of CROSSED) {
  const notification = record({ severity, level });
  const panel = panelTone(notification);
  const card = cardTone(notification);
  const toast = toastTone(notification);
  assert(
    panel === card,
    `one record must tone the same on both surfaces: panel painted ${panel}, chat card painted ${card} (severity ${severity}, level ${level})`,
  );
  assert(
    panel === toast,
    `the toast must share that one tone: panel painted ${panel}, toast painted ${toast} (severity ${severity}, level ${level})`,
  );
  assert(
    panel === expected,
    `the surviving axis is the level, so ${level} must paint ${expected}, got ${panel}`,
  );
}

// The fixtures above only mean something if the tones they choose between are
// actually different tones.
const distinct = new Set(
  (["failure", "action_needed", "verbose"] as Level[]).map((level) =>
    panelTone(record({ level })),
  ),
);
assert(
  distinct.size === 3,
  "the three levels must paint three distinct tones for agreement to be provable",
);

console.log(`tone_axis_agreement tests: ${checked} assertions passed`);
