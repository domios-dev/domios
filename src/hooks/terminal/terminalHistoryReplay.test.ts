import { claimInitialTerminalHistory } from "./terminalHistoryReplay";

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

const firstPane = `pane-history-${Date.now()}-first`;
const secondPane = `pane-history-${Date.now()}-second`;

assert(claimInitialTerminalHistory(firstPane), "first PTY for a pane hydrates history");
assert(!claimInitialTerminalHistory(firstPane), "replacement PTY does not replay pane history again");
assert(claimInitialTerminalHistory(secondPane), "another pane hydrates its own history");

console.log("terminal history replay ownership checks passed");
