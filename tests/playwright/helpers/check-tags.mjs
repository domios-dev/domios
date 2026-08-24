import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { delimiter, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { providerGateFindings } from "./provider-gate-rule.mjs";
import { helperSourceFiles, scanTimerViolations } from "./timer-rules.mjs";

const suitesDir = fileURLToPath(new URL("../suites", import.meta.url));
const helpersDir = fileURLToPath(new URL(".", import.meta.url));
const baselinePath = fileURLToPath(new URL("./check-tags-baseline.txt", import.meta.url));
const tagRe = /@(smoke|golden|perf|live|stub|quarantine)/;
const quarantineTaskRe = /(?:task_[a-z0-9]+|#[0-9]+)/i;
const strictEnv = process.env.PW_LINT_STRICT ?? process.env.PLAYWRIGHT_STRICT_LINT;
const strictEnabled = !/^(0|false|no)$/i.test(strictEnv ?? "");
const actionMethodsWithForce = new Set([
  "check",
  "click",
  "dblclick",
  "dragTo",
  "hover",
  "tap",
  "uncheck",
]);
const violations = [];

function specs(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...specs(path));
    } else if (entry.isFile() && entry.name.endsWith(".spec.ts")) {
      out.push(path);
    }
  }
  return out;
}

function scopedSpecs() {
  const scope = process.env.PW_LINT_SCOPE;
  if (!scope) {
    return specs(suitesDir);
  }

  return scope
    .split(delimiter)
    .filter(Boolean)
    .flatMap((entry) => {
      const path = isAbsolute(entry) ? entry : resolve(process.cwd(), entry);
      const stat = statSync(path);
      if (stat.isDirectory()) {
        return specs(path);
      }
      return stat.isFile() && path.endsWith(".spec.ts") ? [path] : [];
    });
}

function toLabel(file) {
  return relative(process.cwd(), file).split("\\").join("/");
}

function fileLocation(file, line) {
  return toLabel(file) + ":" + line;
}

function location(source, file, node) {
  const pos = source.getLineAndCharacterOfPosition(node.getStart(source));
  return fileLocation(file, pos.line + 1);
}

function titleFromArg(node) {
  if (!node) {
    return null;
  }
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
    return node.text;
  }
  return null;
}

function isTestCall(node) {
  return ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "test";
}

/// Names called inside `node`, plus whether `expect` itself is called there.
function callsWithin(node) {
  const called = new Set();
  let expectsDirectly = false;

  function visit(child) {
    if (ts.isCallExpression(child) && ts.isIdentifier(child.expression)) {
      if (child.expression.text === "expect") {
        expectsDirectly = true;
      } else {
        called.add(child.expression.text);
      }
    }
    ts.forEachChild(child, visit);
  }

  ts.forEachChild(node, visit);
  return { called, expectsDirectly };
}

/// Local functions that assert, directly or by calling another that does. A test
/// delegating its checks to one of these is asserting; requiring a literal
/// `expect` in the test body would flag real coverage and reward inlining it.
function assertingHelpers(source) {
  const bodies = new Map();
  function collect(node) {
    if (ts.isFunctionDeclaration(node) && node.name && node.body) {
      bodies.set(node.name.text, callsWithin(node.body));
    } else if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer &&
      (ts.isArrowFunction(node.initializer) || ts.isFunctionExpression(node.initializer))) {
      bodies.set(node.name.text, callsWithin(node.initializer));
    }
    ts.forEachChild(node, collect);
  }
  collect(source);

  const asserting = new Set();
  for (const [name, body] of bodies) {
    if (body.expectsDirectly) asserting.add(name);
  }
  let grew = true;
  while (grew) {
    grew = false;
    for (const [name, body] of bodies) {
      if (asserting.has(name)) continue;
      for (const callee of body.called) {
        if (asserting.has(callee)) {
          asserting.add(name);
          grew = true;
          break;
        }
      }
    }
  }
  return asserting;
}

function hasExpect(node, asserting) {
  const { called, expectsDirectly } = callsWithin(node);
  if (expectsDirectly) {
    return true;
  }
  for (const callee of called) {
    if (asserting.has(callee)) {
      return true;
    }
  }
  return false;
}

function isTrueLiteral(node) {
  return node.kind === ts.SyntaxKind.TrueKeyword;
}

function propertyNameText(name) {
  if (ts.isIdentifier(name) || ts.isStringLiteral(name) || ts.isNumericLiteral(name)) {
    return name.text;
  }
  return null;
}

function isPlaywrightForceOption(node) {
  if (!ts.isPropertyAssignment(node) || propertyNameText(node.name) !== "force" || !isTrueLiteral(node.initializer)) {
    return false;
  }

  const objectLiteral = node.parent;
  if (!ts.isObjectLiteralExpression(objectLiteral)) {
    return false;
  }

  const call = objectLiteral.parent;
  if (!ts.isCallExpression(call) || !ts.isPropertyAccessExpression(call.expression)) {
    return false;
  }

  return actionMethodsWithForce.has(call.expression.name.text);
}

function checkFile(file) {
  const text = readFileSync(file, "utf8");
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const asserting = assertingHelpers(source);

  function add(node, reason) {
    violations.push(location(source, file, node) + " " + reason);
  }

  function hasJustifiedSleepComment(node) {
    const start = node.getFullStart();
    const lineStart = text.lastIndexOf("\n", start) + 1;
    const previousLineStart = text.lastIndexOf("\n", Math.max(0, lineStart - 2)) + 1;
    const prefix = text.slice(previousLineStart, node.getStart(source));
    return /PW_JUSTIFIED_SLEEP:\s+\S/.test(prefix);
  }

  function visit(node) {
    if (isTestCall(node)) {
      const title = titleFromArg(node.arguments[0]);
      if (title !== null && !tagRe.test(title)) {
        add(node, "Playwright test title is missing a project tag.");
      }
      if (title !== null && /@quarantine/.test(title) && !quarantineTaskRe.test(title)) {
        add(node, "Quarantined Playwright test title must include task_<id> or #<issue>.");
      }

      if (strictEnabled && !hasExpect(node, asserting)) {
        add(node, "Playwright test has no expect() assertion.");
      }
    }

    if (strictEnabled) {
      if (ts.isPropertyAccessExpression(node) && node.name.text === "waitForTimeout") {
        if (!hasJustifiedSleepComment(node)) {
          add(node, "waitForTimeout is banned; use web-first assertions or pollUntil.");
        }
      }

      if (
        (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) &&
        node.text === "networkidle"
      ) {
        add(node, "networkidle waits are banned; wait for the specific UI/API state.");
      }

      if (isPlaywrightForceOption(node)) {
        add(node, "Playwright force: true is banned; wait for an actionable element instead.");
      }

      if (
        ts.isNewExpression(node) &&
        ts.isIdentifier(node.expression) &&
        node.expression.text === "ApiClient"
      ) {
        add(node, "Raw new ApiClient() is banned in specs; use the api fixture.");
      }

      if (
        ts.isCallExpression(node) &&
        ts.isIdentifier(node.expression) &&
        node.expression.text === "connectUi"
      ) {
        add(node, "Raw connectUi() is banned in specs; use the ui fixture.");
      }
    }

    ts.forEachChild(node, visit);
  }

  visit(source);

  if (strictEnabled) {
    for (const finding of providerGateFindings(text, file)) {
      violations.push(fileLocation(file, finding.line) + " " + finding.reason);
    }
  }
}

for (const file of scopedSpecs()) {
  checkFile(file);
}

// helpers/ hid the harness's own sleeps from both checkers; the timer rules
// walk it under the same strict switch and the same baseline file.
if (strictEnabled) {
  for (const file of helperSourceFiles(helpersDir)) {
    violations.push(...scanTimerViolations(readFileSync(file, "utf8"), toLabel(file)));
  }
}

if (violations.length) {
  const baseline = existsSync(baselinePath)
    ? readFileSync(baselinePath, "utf8").split(/\r?\n/).filter(Boolean)
    : [];
  const baselineSet = new Set(baseline);
  const violationSet = new Set(violations);
  const newViolations = violations.filter((violation) => !baselineSet.has(violation));
  const staleBaseline = baseline.filter((violation) => !violationSet.has(violation));
  if (baseline.length > 0 && newViolations.length === 0 && staleBaseline.length === 0) {
    process.exit(0);
  }
  console.error("Playwright lint violations:");
  console.error((newViolations.length > 0 ? newViolations : violations).join("\n"));
  if (staleBaseline.length > 0) {
    console.error("\nStale Playwright lint baseline entries:");
    console.error(staleBaseline.join("\n"));
  }
  process.exit(1);
}
