import { anchoredPanelPosition } from "./NotificationBell";

let checked = 0;

function assert(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

// A phone-width viewport with the bell near the right edge: the panel grows
// leftward from its anchor and used to run off the left edge of the screen.
{
  const viewport = 390;
  const bell = { bottom: 44, right: 300 };
  const { right, maxWidth } = anchoredPanelPosition(bell, viewport);
  assert(right === 90, `panel is anchored to the bell, got right=${right}`);
  assert(
    right + maxWidth <= viewport,
    `the panel must fit on screen: right=${right} maxWidth=${maxWidth} viewport=${viewport}`,
  );
  assert(viewport - right - maxWidth >= 0, "the left edge never goes negative");
}

// A desktop viewport leaves room for the panel's natural width.
{
  const viewport = 1600;
  const { right, maxWidth } = anchoredPanelPosition({ bottom: 44, right: 1500 }, viewport);
  assert(right === 100, `desktop anchor is unchanged, got right=${right}`);
  assert(maxWidth >= 560, `a wide viewport must still allow the expanded panel, got ${maxWidth}`);
}

// An anchor flush against the right edge still keeps a gutter.
{
  const viewport = 390;
  const { right, maxWidth } = anchoredPanelPosition({ bottom: 44, right: 390 }, viewport);
  assert(right === 8, `a flush anchor keeps the edge gutter, got right=${right}`);
  assert(right + maxWidth <= viewport, "a flush anchor still fits on screen");
}

console.log(`notification panel anchor: ${checked} assertions passed`);
