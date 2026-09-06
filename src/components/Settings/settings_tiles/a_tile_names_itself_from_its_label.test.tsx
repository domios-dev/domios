// The tile shape makes two accessibility mistakes easy: naming the control from
// the decorative switch inside it, and rendering a second focusable switch in

import { NotificationsOverview } from "../NotificationsTab";
import {
  TILE,
  assert,
  check,
  configWith,
  fail,
  focusableWithin,
  mount,
  pass,
  setupDom,
  tiles,
  withDaemon,
} from "./harness";

setupDom();

async function open() {
  return mount(
    <NotificationsOverview
      config={configWith()}
      onUpdate={() => {}}
      onUpdateFullConfig={() => {}}
    />,
  );
}

/** 1. Every tile is one switch, named by its label, with the drawn switch out
 *  of the accessibility tree. */
async function everyTileIsOneNamedSwitch() {
  await withDaemon(async () => {
    const view = await open();
    const drawn = tiles(view.host);
    assert(drawn.length > 0, "the panel draws tiles, so these assertions mean something");

    for (const tile of drawn) {
      const name = tile.getAttribute("aria-label");
      assert(name, "every tile names itself");
      assert(tile.getAttribute("role") === "switch", `"${name}" is a switch`);
      assert(tile.getAttribute("type") === "button", `"${name}" is a button, not a form control`);
      assert(
        tile.getAttribute("aria-checked") === "true" || tile.getAttribute("aria-checked") === "false",
        `"${name}" reports a state`,
      );
      assert(
        (tile.textContent ?? "").includes(name),
        `"${name}" is written inside the control it names`,
      );

      const graphic = tile.querySelector('[aria-hidden="true"]');
      assert(graphic, `"${name}" draws a switch`);
      assert(
        graphic.getAttribute("role") === null && graphic.getAttribute("aria-label") === null,
        `the drawn switch inside "${name}" is decoration, never a second named control`,
      );
      assert(
        (graphic.textContent ?? "") === "",
        `the name of "${name}" can only come from its label, never from the drawn switch`,
      );

      const inner = focusableWithin(tile);
      assert(
        inner.length === 0,
        `"${name}" is the only thing a keyboard stops on, found ${inner.length} more`,
      );
    }
    await view.done();
  });
}

/** 2. A hint explains the answer; it is never part of the question. A tile that
 *  carries one still names itself from its label alone. */
async function aHintIsNeverPartOfTheName() {
  await withDaemon(async () => {
    const view = await open();
    const hinted = tiles(view.host).filter(
      (tile) => (tile.textContent ?? "").length > (tile.getAttribute("aria-label") ?? "").length,
    );
    assert(hinted.length > 0, "some tile carries more words than its name, or this proves nothing");

    for (const tile of hinted) {
      const name = tile.getAttribute("aria-label");
      assert(name, "the hinted tile still names itself");
      assert(
        (tile.textContent ?? "").startsWith(name),
        `"${name}" reads as its label first and its explanation after`,
      );
      const explanation = (tile.textContent ?? "").slice(name.length);
      assert(
        explanation.length > 0 && !name.includes(explanation),
        `the explanation under "${name}" is rendered in the tile and kept out of its name`,
      );
    }
    await view.done();
  });
}

/** 3. Nothing in the panel is a tile-shaped control that forgot to be one: every
 *  switch outside a picker is a tile, so there is no second switch grammar. */
async function thereIsOnlyOneSwitchGrammar() {
  await withDaemon(async () => {
    const view = await open();
    const switches = Array.from(view.host.querySelectorAll('button[role="switch"]'));
    const drawn = tiles(view.host);
    assert(
      switches.length === drawn.length,
      `every switch in the panel is a tile, got ${switches.length} switches and ${drawn.length} tiles`,
    );
    assert(
      switches.every((control) => control.matches(TILE)),
      "and none of them is a control of its own",
    );
    await view.done();
  });
}

async function main() {
  await check("settings_tiles: every tile is one named switch", everyTileIsOneNamedSwitch);
  await check("settings_tiles: a hint is never part of the name", aHintIsNeverPartOfTheName);
  await check("settings_tiles: there is only one switch grammar", thereIsOnlyOneSwitchGrammar);
  pass("settings_tiles — a tile names itself from its label");
}

main().catch(fail);
