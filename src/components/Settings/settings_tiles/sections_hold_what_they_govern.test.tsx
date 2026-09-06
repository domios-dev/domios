// The panel was answering three unrelated questions in one flat list. It is
// three sections now, and — the part that matters — a level's own controls are

import { NotificationsOverview } from "../NotificationsTab";
import { levelLabel } from "../../../lib/notifyLevels";
import { SOUND_CHOICE_LABEL } from "../../../lib/notificationSound";
import {
  destinationCellLabel,
  destinationColumns,
  localDestinations,
  pushDestinations,
  pushPickerLabel,
} from "../../../lib/notifyDestinations";
import {
  CHANNEL,
  LEVEL_CARD,
  SECTION,
  assert,
  check,
  configWith,
  fail,
  levelRows,
  mount,
  namedTile,
  pass,
  sectionName,
  sectionNames,
  setupDom,
  tiles,
  withDaemon,
} from "./harness";

setupDom();

const COLUMNS = destinationColumns([CHANNEL]);
const LOCAL = localDestinations(COLUMNS);
const PUSH = pushDestinations(COLUMNS);
/** One tile per locally drawn surface, plus the level's own chime. */
const TILES_PER_LEVEL = LOCAL.length + 1;

const SURFACES = "Surfaces";
const LEVELS = "What each level may interrupt";
const AUDIENCE = `Who ${CHANNEL.label} writes to`;

async function open() {
  return mount(
    <NotificationsOverview
      config={configWith()}
      onUpdate={() => {}}
      onUpdateFullConfig={() => {}}
    />,
  );
}

function sectionTitled(host: Element, title: string): Element {
  const found = Array.from(host.querySelectorAll(SECTION)).find(
    (section) => sectionName(section) === title,
  );
  assert(found, `the panel has a "${title}" section, got ${JSON.stringify(sectionNames(host))}`);
  return found;
}

/** 1. Three sections, each named, each holding the question it asks about. */
async function thePanelIsThreeNamedSections() {
  await withDaemon(async () => {
    const view = await open();
    const names = sectionNames(view.host);
    assert(
      JSON.stringify(names) === JSON.stringify([SURFACES, LEVELS, AUDIENCE]),
      `three sections in the order the panel reads, got ${JSON.stringify(names)}`,
    );

    const surfaces = sectionTitled(view.host, SURFACES);
    assert(
      tiles(surfaces).length === 3,
      `the surfaces section holds its three switches, got ${tiles(surfaces).length}`,
    );

    const levels = sectionTitled(view.host, LEVELS);
    assert(
      levels.querySelectorAll(LEVEL_CARD).length === levelRows().length,
      "the levels section holds one card per shipped level",
    );

    const audience = sectionTitled(view.host, AUDIENCE);
    assert(
      audience.querySelector('[data-testid="settings-channel-audience"]'),
      "the audience section holds the channel's own recipients",
    );

    // Nothing leaks between them: no section holds another section's controls.
    assert(
      surfaces.querySelector(LEVEL_CARD) === null,
      "a level card is never drawn inside the surfaces section",
    );
    assert(
      tiles(levels).every((tile) => !surfaces.contains(tile)),
      "and a level's switches are never drawn inside the surfaces section",
    );
    await view.done();
  });
}

/** 2. A level CONTAINS its own controls. Every switch that names the level, and
 *  the level's push picker, is a descendant of that level's card — not a
 *  sibling sitting next to it. */
async function aLevelCardContainsItsOwnControls() {
  await withDaemon(async () => {
    const view = await open();
    const levels = sectionTitled(view.host, LEVELS);
    const cards = Array.from(levels.querySelectorAll(LEVEL_CARD));
    assert(cards.length === levelRows().length, "every shipped level got a card");

    for (const card of cards) {
      const level = card.getAttribute("data-notification-level");
      assert(level, "every card names the level it governs");
      const label = levelLabel(level);

      const inside = tiles(card);
      assert(
        inside.length === TILES_PER_LEVEL,
        `${label} holds one switch per surface plus its chime, got ${inside.length}`,
      );

      const expected = [
        ...LOCAL.map((destination) => destinationCellLabel(label, destination.label)),
        destinationCellLabel(label, SOUND_CHOICE_LABEL),
      ];
      for (const name of expected) {
        const tile = namedTile(view.host, name);
        assert(card.contains(tile), `"${name}" is inside the ${label} card, not beside it`);
      }

      if (PUSH.length > 0) {
        const picker = view.host.querySelector(
          `[data-testid="settings-destination-picker-trigger"][aria-label="${pushPickerLabel(label)}"]`,
        );
        assert(picker, `${label} offers its push picker`);
        assert(card.contains(picker), `and the picker is inside the ${label} card`);
      }

      // The containment claim is only worth something if a neighbour's controls
      // are genuinely outside: no other card holds any of this level's switches.
      for (const other of cards) {
        if (other === card) continue;
        assert(
          inside.every((tile) => !other.contains(tile)),
          `no other level's card holds ${label}'s switches`,
        );
      }
    }
    await view.done();
  });
}

async function main() {
  await check("settings_tiles: the panel is three named sections", thePanelIsThreeNamedSections);
  await check(
    "settings_tiles: a level card contains its own controls",
    aLevelCardContainsItsOwnControls,
  );
  pass("settings_tiles — sections hold what they govern");
}

main().catch(fail);
