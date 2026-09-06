// `severity` survives on the record as a wire and dedupe axis — the Rust
// notification identity is built from it — but it is inert for tone: it never
import type { Notification } from "../../lib/types";
import { notificationLevel } from "../../lib/notifyLevels";
import {
  cardTone,
  panelTone,
  record,
  toastSeverity,
  toastTone,
} from "./toneAxisSurfaces";

let checked = 0;
function assert(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

type Level = NonNullable<Notification["level"]>;

const SEVERITIES: Array<Notification["severity"]> = ["Info", "Warning", "Error"];
const LEVELS: Level[] = ["verbose", "action_needed", "failure"];

// 1. Holding the level still and moving severity moves nothing on any surface.
for (const level of LEVELS) {
  const painted = SEVERITIES.map((severity) => {
    const notification = record({ severity, level });
    return [panelTone(notification), cardTone(notification), toastTone(notification)].join("/");
  });
  assert(
    new Set(painted).size === 1,
    `severity must not tone anything at level ${level}, got ${painted.join(" vs ")}`,
  );
}

// 2. Holding severity still and moving the level moves every surface.
for (const severity of SEVERITIES) {
  const painted = LEVELS.map((level) => panelTone(record({ severity, level })));
  assert(
    new Set(painted).size === LEVELS.length,
    `the level must be what tones, even at a fixed severity ${severity}, got ${painted.join(" vs ")}`,
  );
}

// 3. Severity writes no level: an unlevelled record stays unlevelled however
//    loud its severity is, so nothing derives one axis from the other.
for (const severity of SEVERITIES) {
  assert(
    notificationLevel(record({ severity })) === null,
    `severity ${severity} must not write a level onto an unlevelled record`,
  );
  assert(
    notificationLevel(record({ severity, level: "verbose" })) === "verbose",
    `a declared level must be reported verbatim, whatever severity ${severity} says`,
  );
}

// 4. Retained, not removed: the record still carries severity and the toast
//    still reports it, as identity rather than as tone.
assert(
  toastSeverity(record({ severity: "Error", level: "verbose" })) === "Error",
  "severity must survive on the record as the identity axis it always was",
);

console.log(`tone_axis_severity_inert tests: ${checked} assertions passed`);
