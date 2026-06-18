import { readdirSync, readFileSync, statSync } from "node:fs";
import { delimiter, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const suitesDir = fileURLToPath(new URL("../suites", import.meta.url));
const tagRe = /@(smoke|golden|quarantine)/;
const strictEnv = process.env.PW_LINT_STRICT ?? process.env.PLAYWRIGHT_STRICT_LINT;
const strictEnabled = /^(1|true|yes)$/i.test(strictEnv ?? "");
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

function location(source, file, node) {
  const pos = source.getLineAndCharacterOfPosition(node.getStart(source));
  return relative(process.cwd(), file) + ":" + (pos.line + 1);
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

function hasExpect(node) {
  let found = false;

  function visit(child) {
    if (found) {
      return;
    }
    if (ts.isCallExpression(child) && ts.isIdentifier(child.expression) && child.expression.text === "expect") {
      found = true;
      return;
    }
    ts.forEachChild(child, visit);
  }

  ts.forEachChild(node, visit);
  return found;
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

  function add(node, reason) {
    violations.push(location(source, file, node) + " " + reason);
  }

  function visit(node) {
    if (isTestCall(node)) {
      const title = titleFromArg(node.arguments[0]);
      if (title !== null && !tagRe.test(title)) {
        add(node, "Playwright test title is missing @smoke, @golden, or @quarantine.");
      }

      if (strictEnabled && !hasExpect(node)) {
        add(node, "Playwright test has no expect() assertion.");
      }
    }

    if (strictEnabled) {
      if (ts.isPropertyAccessExpression(node) && node.name.text === "waitForTimeout") {
        add(node, "waitForTimeout is banned; use web-first assertions or pollUntil.");
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
}

for (const file of scopedSpecs()) {
  checkFile(file);
}

if (violations.length) {
  console.error("Playwright lint violations:");
  console.error(violations.join("\n"));
  process.exit(1);
}
