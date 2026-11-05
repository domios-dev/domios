import assert from "node:assert/strict";
import { isAbsolute } from "node:path";
import type { Operation, RuntimeContext, RuntimePlan, RuntimeReduction } from "@codeterm/plugin-sdk";

const identifier=(value:string):boolean=>value.length > 0 && Buffer.byteLength(value) <= 4096 && !value.includes("\0");
function ownsThread(ctx:RuntimeContext, thread:string):boolean {
  if (!ctx.threadId) return false;
  const seen=new Set<string>();
  let current=thread;
  while (current !== ctx.threadId) {
    if (seen.has(current)) return false;
    seen.add(current);
    const parent=ctx.threadParents.find(parent=>parent.threadId === current);
    if (!parent) return false;
    current=parent.parentThreadId;
  }
  return true;
}

export function assertRuntimeOperation(ctx:RuntimeContext, op:Operation):void {
  assert.ok(ctx.activeTurns.length <= 32,"too many active turns");
  const activeThreads=new Set<string>();
  for (const turn of ctx.activeTurns) {
    assert.ok(identifier(turn.threadId) && identifier(turn.turnId) && !activeThreads.has(turn.threadId),"invalid active turn roster");
    activeThreads.add(turn.threadId);
    if (turn.threadId === ctx.threadId) assert.equal(turn.parentThreadId ?? null,null,"root turn has a parent");
    else {
      assert.ok(ownsThread(ctx,turn.threadId),"active descendant has no known ancestry");
      const parent=ctx.threadParents.find(parent=>parent.threadId === turn.threadId)!;
      assert.ok(turn.parentThreadId === undefined || turn.parentThreadId === parent.parentThreadId,"active descendant changes proven parent");
    }
  }
  if (op.kind === "openThread" || op.kind === "resumeThread" || op.kind === "submitTurn") {
    const directories=op.settings.additionalDirectories ?? [];
    assert.ok(directories.length <= 32 && new Set(directories).size === directories.length
      && directories.every(directory=>identifier(directory) && isAbsolute(directory)),"invalid additional runtime directories");
  }
  switch (op.kind) {
    case "forkThread":
      assert.ok(!ctx.threadId && !ctx.activeTurnId && ctx.activeTurns.length === 0
        && identifier(op.sourceThreadId) && identifier(op.turnId),"fork requires an unbound runtime and valid source boundary");
      return;
    case "backgroundProcesses":
      assert.ok(ownsThread(ctx,op.threadId),"background operation targets an unowned thread");
      if (op.action.kind === "terminate") assert.ok(identifier(op.action.processId),"invalid background process id");
      if (op.action.kind === "list" && op.action.cursor != null) assert.ok(identifier(op.action.cursor),"invalid background process cursor");
      return;
    case "resumeThread":
      assert.ok(ctx.threadId === undefined || ctx.threadId === op.threadId,"resume retargets thread");
      assert.ok(identifier(op.threadId),"empty resume target");
      return;
    case "answer":
      assert.equal(ctx.pendingRequest?.requestId,op.requestId,"answer targets unknown request");
      return;
    case "openThread":
      assert.equal(ctx.threadId ?? null,null,"open replaces bound thread");
      return;
    case "initialize": return;
    case "control":
      if (op.threadId !== undefined) assert.equal(op.threadId,ctx.threadId,"operation retargets thread");
      return;
    case "readHistory":
      assert.ok(op.threadId === ctx.threadId || ownsThread(ctx,op.threadId),"operation retargets thread");
      return;
    case "submitTurn":
    case "interrupt":
    case "answerQuestion":
      assert.equal(op.threadId,ctx.threadId,"operation retargets thread");
      break;
  }
  if (op.kind === "submitTurn" || op.kind === "interrupt") {
    if (op.kind === "submitTurn") assert.ok(identifier(op.sendId),"invalid send id");
    assert.equal(op.activeTurnId ?? null,ctx.activeTurnId ?? null,`${op.kind === "submitTurn" ? "submit" : "interrupt"} retargets turn`);
  } else if (op.kind === "answerQuestion") {
    assert.ok(identifier(op.itemId) && op.answers.length > 0 && op.answers.length <= 32,"invalid question answer");
    const ids=new Set<string>();
    for (const answer of op.answers) {
      assert.ok(identifier(answer.questionId) && !ids.has(answer.questionId) && answer.values.length <= 128
        && answer.values.every(identifier) && (answer.text === undefined || Buffer.byteLength(answer.text) <= 65536),"invalid question answer values");
      ids.add(answer.questionId);
    }
  }
}

export function assertRuntimePlan(ctx:RuntimeContext, op:Operation, plan:RuntimePlan):void {
  if (plan.action.kind === "refused") return;
  if (op.kind === "answerQuestion") assert.equal(plan.action.kind,"questionAnswer","question answer must be local formatting");
  if (plan.action.kind === "local")
    assert.ok(op.kind === "resumeThread" && ctx.threadId !== undefined && op.threadId === ctx.threadId,"local action requires the bound resume thread");
  if (plan.action.kind === "questionAnswer")
    assert.equal(op.kind,"answerQuestion","question answer action outside answerQuestion");
}

export function assertRuntimeReduction(reduction:RuntimeReduction):void {
  assert.ok(reduction.events.length <= 1024,"runtime reduction exceeds 1024 events");
  assert.ok(Buffer.byteLength(JSON.stringify(reduction)) <= 4*1024*1024-1024,"runtime reduction exceeds 4 MiB minus serialization margin");
  assert.ok(Buffer.byteLength(JSON.stringify(reduction.state)) <= 256*1024,"runtime state exceeds 256 KiB");
}
