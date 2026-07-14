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

const abandoned = claimInitialTerminalHistory(firstPane);
assert(abandoned != null, "first PTY for a pane owns the initial history claim");
assert(claimInitialTerminalHistory(firstPane) == null, "parallel mounts do not duplicate history work");
abandonInitialTerminalHistory(abandoned);

const completed = claimInitialTerminalHistory(firstPane);
assert(completed != null, "a cancelled mount releases the history claim");
completeInitialTerminalHistory(completed);
assert(claimInitialTerminalHistory(firstPane) == null, "completed history is never replayed again");

const second = claimInitialTerminalHistory(secondPane);
assert(second != null, "another pane owns an independent history claim");

console.log("terminal history replay ownership checks passed");
