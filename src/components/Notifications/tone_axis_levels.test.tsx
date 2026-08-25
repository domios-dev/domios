// Run: npm run test:unit -- tone_axis
//
// Each level paints from its own existing product token, asserted through the
// token reference rather than a literal colour, so a theme change moves the
// notification surfaces with everything else.
import type { Notification } from "../../lib/types";
import { levelTone } from "../../lib/notifyLevels";
import {
  cardMarkup,
  cardTone,
  panelMarkup,
  panelTone,
  record,
  toastTone,
} from "./toneAxisSurfaces";

let checked = 0;
function assert(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

type Level = NonNullable<Notification["level"]>;

const LEVELS: Array<[Level, string]> = [
  ["verbose", "--ct-muted-fg"],
  ["action_needed", "--ct-yellow"],
  ["failure", "--ct-red"],
];

for (const [level, token] of LEVELS) {
  assert(
    levelTone(level) === `var(${token})`,
    `${level} must tone from the existing ${token} token, got ${levelTone(level)}`,
  );
  const notification = record({ level });
  for (const [surface, painted] of [
    ["panel", panelTone(notification)],
    ["chat card", cardTone(notification)],
    ["toast", toastTone(notification)],
  ] as const) {
    assert(
      `var(${painted})` === levelTone(level),
      `the ${surface} must paint ${level} from ${token}, got ${painted}`,
    );
  }
  for (const [surface, markup] of [
    ["panel", panelMarkup(notification)],
    ["chat card", cardMarkup(notification)],
  ] as const) {
    assert(
      !/#[0-9a-fA-F]{3,8}/.test(markup),
      `the ${surface} must not resolve a token to a literal colour for ${level}`,
    );
  }
}

console.log(`tone_axis_levels tests: ${checked} assertions passed`);
