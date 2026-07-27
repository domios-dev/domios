import {
  abandonInitialTerminalHistory,
  claimInitialTerminalHistory,
  completeInitialTerminalHistory,
} from "./terminalHistoryReplay";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

const firstTab = `tab-history-${Date.now()}-first`;
const secondTab = `tab-history-${Date.now()}-second`;
const firstTerminal = {};

const abandoned = claimInitialTerminalHistory(firstTab, firstTerminal);
assert(abandoned != null, "first PTY for a Tab owns the initial history claim");
assert(claimInitialTerminalHistory(firstTab, firstTerminal) == null, "one terminal cannot duplicate history work");
const replacement = claimInitialTerminalHistory(firstTab, {});
assert(replacement != null, "a replacement terminal can hydrate while the old renderer disposes");
abandonInitialTerminalHistory(abandoned);
abandonInitialTerminalHistory(replacement);

const completed = claimInitialTerminalHistory(firstTab, firstTerminal);
assert(completed != null, "a cancelled mount releases the history claim");
completeInitialTerminalHistory(completed);
assert(claimInitialTerminalHistory(firstTab, firstTerminal) == null, "one terminal instance replays history once");
assert(claimInitialTerminalHistory(firstTab, {}) != null, "a remounted terminal receives its own history replay");

const second = claimInitialTerminalHistory(secondTab, {});
assert(second != null, "another Tab owns an independent history claim");

console.log("terminal history replay ownership checks passed");
