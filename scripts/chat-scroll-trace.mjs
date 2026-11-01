// Caller: manual DEV phone diagnostics
import { sameAnchorLayoutResidual } from "./chat-scroll-residual.mjs";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const edgeNames = ["touchstart", "touchend", "scroll", "scrollend", "gesture", "write", "absorb", "resize", "clientHeight", "visualViewport", "capabilities", "jump_ignored_owned", "fold"];
export function decodeChatScrollTrace(log, threshold = 8) {
  const groups = new Map();
  for (const line of log.split(/\r?\n/)) {
    const match = line.match(/\[chat-scroll\] client=(\S+) build_sha=(.*?) (\{.*\})$/);
    if (!match) continue;
    let chat;
    try { chat = JSON.parse(match[3]); } catch { continue; }
    const chunk = chat.state;
    if (chat.edge !== "trace" || !chunk || !Number.isInteger(chunk.index) || !Number.isInteger(chunk.count)) continue;
    const key = `${match[1]} ${match[2]} ${chunk.gestureId}`;
    if (!groups.has(key)) groups.set(key, new Map());
    groups.get(key).set(chunk.index, chunk);
  }
  return Array.from(groups, ([key, indexed]) => {
    const chunks = Array.from(indexed.values()).sort((a, b) => a.index - b.index);
    const frames = chunks.flatMap(chunk => chunk.frames).sort((a, b) => a[0] - b[0]);
    const edges = chunks.flatMap(chunk => chunk.edges).sort((a, b) => a[0] - b[0]);
    const jumps = [];
    const residuals = [];
    for (let index = 1; index < frames.length; index++) {
      const before = frames[index - 1], after = frames[index];
      const compensated = edges.some(edge => edge[0] >= before[0] && edge[0] < after[0]
        && (edge[1] === 5 || edge[1] === 12));
      const result = after[0] === before[0] + 1 ? sameAnchorLayoutResidual(
        { anchor: before[10], anchorTop: before[11], scrollTop: before[1] },
        { anchor: after[10], anchorTop: after[11], scrollTop: after[1] }, threshold, compensated) : null;
      if (!result) continue;
      residuals.push({ frame: after[0], ...result });
      const delta = result.compensated ? result.visualDelta : result.residual;
      if (!result.failed) continue;
      jumps.push({ frame: after[0], delta, before, after,
        context: edges.filter(edge => edge[0] >= after[0] - 5 && edge[0] <= after[0])
          .map(([frame, kind, ...values]) => ({ frame, edge: edgeNames[kind] ?? "unknown", values })) });
    }
    return { key, complete: chunks.length === chunks[0].count, received: chunks.length, expected: chunks[0].count,
      droppedFrames: chunks[0].droppedFrames, droppedEdges: chunks[0].droppedEdges, droppedBatches: chunks[0].droppedBatches,
      writesWhileOwned: edges.filter(edge => edge[1] === 5 && edge[4] === 1).length,
      ignoredOwnedJumps: edges.filter(edge => edge[1] === 11).length, residuals, jumps };
  });
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (!process.argv[2]) throw new Error("Usage: node scripts/chat-scroll-trace.mjs LOG [jump-pixels=8]");
  const decoded = decodeChatScrollTrace(readFileSync(process.argv[2], "utf8"), Number(process.argv[3] ?? 8));
  console.log(JSON.stringify(decoded, null, 2));
  process.exitCode = decoded.length === 0 || decoded.some(trace => !trace.complete || trace.droppedFrames || trace.droppedEdges
    || trace.droppedBatches || trace.jumps.length || trace.writesWhileOwned) ? 1 : 0;
}
