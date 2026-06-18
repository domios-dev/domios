import { readdirSync, readFileSync } from "node:fs";
import { relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import ts from "typescript";

const helpersDir = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = resolve(helpersDir, "../../..");
const defaultSuitesDir = resolve(helpersDir, "../suites");

const brittleSelectorKeys = ["data-testid", "data-test", "data-cy", "aria-label", "role="];
const actionMethods = new Set([
  "check",
  "click",
  "dblclick",
  "dragTo",
  "fill",
  "hover",
  "press",
  "selectOption",
  "setChecked",
  "setInputFiles",
  "tap",
  "type",
  "uncheck",
]);

function toPosix(path) {
  return path.split(sep).join("/");
}

function repoRelative(path) {
  return toPosix(relative(repoRoot, path));
}

function specs(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = resolve(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...specs(path));
    } else if (entry.isFile() && entry.name.endsWith(".spec.ts")) {
      out.push(path);
    }
  }
  return out.sort();
}

function calleeName(node) {
  if (ts.isIdentifier(node)) return node.text;
  if (ts.isPropertyAccessExpression(node)) return node.name.text;
  return "";
}

function isTestCallee(node) {
  if (ts.isIdentifier(node)) return node.text === "test";
  if (!ts.isPropertyAccessExpression(node)) return false;
  const name = node.name.text;
  if (["describe", "beforeAll", "beforeEach", "afterAll", "afterEach"].includes(name)) {
    return false;
  }
  return isTestCallee(node.expression);
}

function literalText(node) {
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
  return null;
}

function findCallback(args) {
  return args.find((arg) => ts.isArrowFunction(arg) || ts.isFunctionExpression(arg));
}

function containsExpect(node) {
  let found = false;
  function visit(child) {
    if (found) return;
    if (ts.isCallExpression(child)) {
      if (ts.isIdentifier(child.expression) && child.expression.text === "expect") {
        found = true;
        return;
      }
      if (
        ts.isPropertyAccessExpression(child.expression) &&
        ts.isIdentifier(child.expression.expression) &&
        child.expression.expression.text === "expect"
      ) {
        found = true;
        return;
      }
    }
    ts.forEachChild(child, visit);
  }
  visit(node);
  return found;
}

function location(sourceFile, node) {
  const pos = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile));
  return { line: pos.line + 1, column: pos.character + 1 };
}

function findOwningTest(tests, node) {
  const start = node.getStart();
  return tests.find((test) => start >= test.start && start <= test.end) ?? null;
}

function selectorIssues(selector) {
  const trimmed = selector.trim();
  const issues = [];
  for (const branch of trimmed.split(",").map((part) => part.trim()).filter(Boolean)) {
    const hasStableKey = brittleSelectorKeys.some((key) => branch.includes(key));
    if (/:nth-(?:child|of-type)\(/.test(branch)) {
      issues.push({
        rule: "brittle-selector:nth",
        message: `selector uses nth-child/nth-of-type: ${branch}`,
      });
    }
    if (/^\.[\w-]/.test(branch) && !hasStableKey) {
      issues.push({
        rule: "brittle-selector:dot-first",
        message: `selector starts from a CSS class without a unique key: ${branch}`,
      });
    }
    const combinators = (branch.match(/\s[>+~]\s|>/g) ?? []).length;
    const descendantSteps = (branch.match(/(?:^|[\s>+~])(?:[.#]?\w|\[)/g) ?? []).length;
    if (!hasStableKey && (combinators >= 2 || descendantSteps >= 4)) {
      issues.push({
        rule: "brittle-selector:deep-css",
        message: `selector is deep CSS without a unique key: ${branch}`,
      });
    }
  }
  return issues;
}

function addFinding(findings, sourceFile, file, node, rule, message, tests, severity = "warn") {
  const loc = location(sourceFile, node);
  const test = findOwningTest(tests, node);
  findings.push({
    rule,
    severity,
    file: repoRelative(file),
    line: loc.line,
    column: loc.column,
    testTitle: test?.title ?? null,
    message,
  });
}

function closestCallExpression(node) {
  let current = node.parent;
  while (current) {
    if (ts.isCallExpression(current)) return current;
    current = current.parent;
  }
  return null;
}

function collectTests(sourceFile) {
  const tests = [];
  function visit(node) {
    if (ts.isCallExpression(node) && isTestCallee(node.expression)) {
      const title = literalText(node.arguments[0]);
      const callback = findCallback([...node.arguments]);
      if (title && callback) {
        tests.push({
          title,
          start: callback.getStart(sourceFile),
          end: callback.end,
          line: location(sourceFile, node).line,
          hasExpect: containsExpect(callback),
        });
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);
  return tests;
}

function analyzeFile(file) {
  const text = readFileSync(file, "utf8");
  const sourceFile = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const tests = collectTests(sourceFile);
  const findings = [];

  for (const test of tests) {
    if (!test.hasExpect) {
      findings.push({
        rule: "missing-expect",
        severity: "error",
        file: repoRelative(file),
        line: test.line,
        column: 1,
        testTitle: test.title,
        message: `test has no expect(): ${test.title}`,
      });
    }
  }

  function visit(node) {
    if (ts.isCallExpression(node)) {
      const name = calleeName(node.expression);
      if (name === "waitForTimeout") {
        addFinding(
          findings,
          sourceFile,
          file,
          node,
          "sleep:waitForTimeout",
          "waitForTimeout is a hard sleep; use web-first assertions or pollUntil",
          tests,
          "error",
        );
      }
      if (ts.isIdentifier(node.expression) && node.expression.text === "setTimeout") {
        addFinding(
          findings,
          sourceFile,
          file,
          node,
          "sleep:setTimeout",
          "bare setTimeout sleep/poll loop in a spec; use pollUntil or an assertion",
          tests,
          "error",
        );
      }
      if (ts.isNewExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "ApiClient") {
        addFinding(
          findings,
          sourceFile,
          file,
          node,
          "inline-fixture:api-client",
          "spec creates ApiClient directly instead of using the api fixture",
          tests,
        );
      }
      if (ts.isIdentifier(node.expression) && node.expression.text === "connectUi") {
        addFinding(
          findings,
          sourceFile,
          file,
          node,
          "inline-fixture:connect-ui",
          "spec calls connectUi() directly instead of using the ui fixture",
          tests,
        );
      }
      if (name === "locator") {
        const selector = literalText(node.arguments[0]);
        if (selector) {
          for (const issue of selectorIssues(selector)) {
            addFinding(findings, sourceFile, file, node.arguments[0], issue.rule, issue.message, tests);
          }
        }
      }
    }

    if (
      ts.isNewExpression(node) &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === "ApiClient"
    ) {
      addFinding(
        findings,
        sourceFile,
        file,
        node,
        "inline-fixture:api-client",
        "spec creates ApiClient directly instead of using the api fixture",
        tests,
      );
    }

    if (ts.isPropertyAssignment(node)) {
      const name = ts.isIdentifier(node.name) || ts.isStringLiteral(node.name) ? node.name.text : "";
      if (name === "force" && node.initializer.kind === ts.SyntaxKind.TrueKeyword) {
        const call = closestCallExpression(node);
        const action = call ? calleeName(call.expression) : "";
        if (actionMethods.has(action)) {
          addFinding(
            findings,
            sourceFile,
            file,
            node,
            "force-action",
            "force: true masks timing/actionability problems",
            tests,
            "error",
          );
        }
      }
    }

    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      if (node.text === "networkidle") {
        addFinding(
          findings,
          sourceFile,
          file,
          node,
          "networkidle",
          "networkidle is brittle in app-style UIs; wait for a concrete UI/API condition",
          tests,
          "error",
        );
      }
    }

    ts.forEachChild(node, visit);
  }
  visit(sourceFile);

  return {
    file: repoRelative(file),
    tests: tests.map((test) => ({
      title: test.title,
      line: test.line,
      hasExpect: test.hasExpect,
      findings: findings.filter((finding) => finding.testTitle === test.title).length,
    })),
    findings,
  };
}

export function runStaticAudit(options = {}) {
  const suitesDir = resolve(options.suitesDir ?? defaultSuitesDir);
  const files = specs(suitesDir);
  const fileResults = files.map(analyzeFile);
  const findings = fileResults.flatMap((result) => result.findings);
  const tests = fileResults.flatMap((result) =>
    result.tests.map((test) => ({
      ...test,
      file: result.file,
    })),
  );
  const ruleCounts = findings.reduce((counts, finding) => {
    counts[finding.rule] = (counts[finding.rule] ?? 0) + 1;
    return counts;
  }, {});

  return {
    generatedAt: new Date().toISOString(),
    suitesDir: repoRelative(suitesDir),
    summary: {
      files: files.length,
      tests: tests.length,
      findings: findings.length,
      errors: findings.filter((finding) => finding.severity === "error").length,
      warnings: findings.filter((finding) => finding.severity === "warn").length,
      ruleCounts,
    },
    tests,
    findings,
  };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const result = runStaticAudit({ suitesDir: process.argv[2] });
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
}
