/**
 * Timer rules for the Playwright harness itself (`helpers/`), where the suite
 * checkers never looked. Suites are policed for `waitForTimeout`; helpers hide
 * their sleeps behind `new Promise(resolve => setTimeout(resolve, N))`, which
 * no rule covered until now.
 */
import { readdirSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";

export const TIMER_RULE_REASONS = {
  raceResolvedByTimer:
    "a timer that RESOLVES a Promise.race treats elapsed time as readiness; let the event resolve and the timer only reject.",
  fixedDelayAwait:
    "awaiting a fixed delay outside a poll loop is a sleep; wait on an observable event with a bounded failure deadline.",
  waitForTimeout: "waitForTimeout is banned; use web-first assertions or pollUntil.",
};

const JUSTIFIED_SLEEP = /PW_JUSTIFIED_SLEEP:\s+\S/;

/** The path coverage itself: every helper the harness runs, tests excluded. */
export function helperSourceFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...helperSourceFiles(path));
    } else if (entry.isFile() && entry.name.endsWith(".ts") && !entry.name.endsWith(".test.ts")) {
      out.push(path);
    }
  }
  return out;
}

function isSetTimeoutCallee(expression) {
  if (ts.isIdentifier(expression)) return expression.text === "setTimeout";
  return ts.isPropertyAccessExpression(expression) && expression.name.text === "setTimeout";
}

function callsIdentifier(node, name) {
  if (ts.isIdentifier(node)) return node.text === name;
  if (ts.isCallExpression(node)) return ts.isIdentifier(node.expression) && node.expression.text === name;
  if (ts.isArrowFunction(node) || ts.isFunctionExpression(node)) {
    const body = singleExpression(node.body);
    return body !== null && callsIdentifier(body, name);
  }
  return false;
}

function singleExpression(body) {
  if (!body) return null;
  if (ts.isBlock(body)) {
    if (body.statements.length !== 1) return null;
    const [only] = body.statements;
    return ts.isExpressionStatement(only) ? only.expression : null;
  }
  return ts.isExpressionStatement(body) ? body.expression : body;
}

/**
 * `new Promise(resolve => setTimeout(resolve, N))` and nothing else. A
 * two-parameter executor is the failure-deadline shape (the timer rejects),
 * which is a proof, not a sleep.
 */
export function isDelayPromise(node) {
  if (!ts.isNewExpression(node)) return false;
  if (!ts.isIdentifier(node.expression) || node.expression.text !== "Promise") return false;
  const executor = node.arguments?.[0];
  if (!executor || (!ts.isArrowFunction(executor) && !ts.isFunctionExpression(executor))) return false;
  if (executor.parameters.length !== 1) return false;
  const parameter = executor.parameters[0].name;
  if (!ts.isIdentifier(parameter)) return false;
  const body = singleExpression(executor.body);
  if (body === null || !ts.isCallExpression(body) || !isSetTimeoutCallee(body.expression)) return false;
  const scheduled = body.arguments[0];
  return Boolean(scheduled) && callsIdentifier(scheduled, parameter.text);
}

function raceElements(node) {
  if (!ts.isCallExpression(node) || !ts.isPropertyAccessExpression(node.expression)) return [];
  const { expression } = node.expression;
  if (!ts.isIdentifier(expression) || expression.text !== "Promise") return [];
  if (node.expression.name.text !== "race") return [];
  const [list] = node.arguments;
  return list && ts.isArrayLiteralExpression(list) ? [...list.elements] : [];
}

function stripThenChain(node) {
  let current = node;
  while (
    ts.isCallExpression(current) &&
    ts.isPropertyAccessExpression(current.expression) &&
    (current.expression.name.text === "then" || current.expression.name.text === "catch")
  ) {
    current = current.expression.expression;
  }
  return current;
}

function insideIterationStatement(node) {
  for (let parent = node.parent; parent; parent = parent.parent) {
    if (ts.isIterationStatement(parent, false)) return true;
  }
  return false;
}

function lineOf(source, node) {
  return source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
}

function annotated(text, source, node) {
  const start = node.getStart(source);
  const lineStart = text.lastIndexOf("\n", start) + 1;
  const previousLineStart = text.lastIndexOf("\n", Math.max(0, lineStart - 2)) + 1;
  return JUSTIFIED_SLEEP.test(text.slice(previousLineStart, start));
}

/**
 * Pure: source text in, `"<label>:<line> <reason>"` out. Same violation string
 * shape as check-tags.mjs so one baseline file covers both walks.
 */
export function scanTimerViolations(text, label) {
  const source = ts.createSourceFile(label, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const violations = [];
  const seen = new Set();

  const add = (node, reason) => {
    if (annotated(text, source, node)) return;
    const entry = `${label}:${lineOf(source, node)} ${reason}`;
    if (seen.has(entry)) return;
    seen.add(entry);
    violations.push(entry);
  };

  function visit(node) {
    for (const element of raceElements(node)) {
      if (isDelayPromise(stripThenChain(element))) {
        add(element, TIMER_RULE_REASONS.raceResolvedByTimer);
      }
    }

    if (ts.isAwaitExpression(node) && isDelayPromise(stripThenChain(node.expression))) {
      if (!insideIterationStatement(node)) add(node, TIMER_RULE_REASONS.fixedDelayAwait);
    }

    if (ts.isPropertyAccessExpression(node) && node.name.text === "waitForTimeout") {
      add(node, TIMER_RULE_REASONS.waitForTimeout);
    }

    ts.forEachChild(node, visit);
  }

  visit(source);
  return violations;
}
