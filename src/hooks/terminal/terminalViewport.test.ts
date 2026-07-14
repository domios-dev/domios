import { captureTerminalViewport, restoreTerminalViewport } from "./terminalViewport";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const calls: string[] = [];
const term = {
  buffer: { active: { type: "normal" as const, viewportY: 140, baseY: 140 } },
  scrollToBottom: () => { calls.push("bottom"); },
  scrollToLine: (line: number) => { calls.push(`line:${line}`); },
};

const following = captureTerminalViewport(term);
assert(following.followOutput, "bottom viewport follows output");
restoreTerminalViewport(term, following);
assert(calls.join(",") === "bottom", "following viewport restores at the bottom");

term.buffer.active.viewportY = 37;
const reading = captureTerminalViewport(term);
assert(!reading.followOutput, "scrolled viewport stops following output");
term.buffer.active.baseY = 20;
restoreTerminalViewport(term, reading);
assert(calls[calls.length - 1] === "line:20", "restoration clamps manual position to retained history");

console.log("terminal viewport checks passed");
