import {
  abandonInitialTerminalHistory,
  claimInitialTerminalHistory,
  completeInitialTerminalHistory,
} from "./terminalHistoryReplay";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

const firstPane = `pane-history-${Date.now()}-first`;
const secondPane = `pane-history-${Date.now()}-second`;
const firstTerminal = {};

const abandoned = claimInitialTerminalHistory(firstPane, firstTerminal);
assert(abandoned != null, "first PTY for a Tab owns the initial history claim");
assert(claimInitialTerminalHistory(firstPane, firstTerminal) == null, "one terminal cannot duplicate history work");
const replacement = claimInitialTerminalHistory(firstPane, {});
assert(replacement != null, "a replacement terminal can hydrate while the old renderer disposes");
abandonInitialTerminalHistory(abandoned);
abandonInitialTerminalHistory(replacement);

const completed = claimInitialTerminalHistory(firstPane, firstTerminal);
assert(completed != null, "a cancelled mount releases the history claim");
completeInitialTerminalHistory(completed);
assert(claimInitialTerminalHistory(firstPane, firstTerminal) == null, "one terminal instance replays history once");
assert(claimInitialTerminalHistory(firstPane, {}) != null, "a remounted terminal receives its own history replay");

const second = claimInitialTerminalHistory(secondPane, {});
assert(second != null, "another Tab owns an independent history claim");

console.log("terminal history replay ownership checks passed");
