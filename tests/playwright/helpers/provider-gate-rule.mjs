/**
 * Static rule: a Playwright test that spawns a real agent must declare where
 * its provider bytes come from — exactly one of "stub" or "live".
 *
 * "stub" is declared by the @stub tag OR by an in-scope stub-transport gate
 * (isStubTransportActive), because a gate is what actually routes the spawn
 * through CODETERM_STUB_AGENT_CMD. "live" is declared by the @live tag.
 * Zero declarations means the test burns provider credit inside a project that
 * claims not to; two means the tags contradict each other.
 */
import ts from "typescript";

/** Provider ids a spawn can name and still reach a real CLI. */
export const SPAWNABLE_PROVIDER_IDS = new Set(["claude", "codex", "opencode", "cursor"]);

export const STUB_GATE_IDENTIFIERS = new Set(["isStubTransportActive"]);

export const PROVIDER_GATE_REASON =
  "Agent-spawning Playwright test must declare exactly one provider source: " +
  "@stub (or an in-scope stub-transport gate) or @live.";

const SPAWN_PATH = "/agents/spawn";
const SPAWN_FIXTURE = "spawnAgent";

function isTestCall(node) {
  return ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "test";
}

function isDescribeCall(node) {
  return (
    ts.isCallExpression(node) &&
    ts.isPropertyAccessExpression(node.expression) &&
    ts.isIdentifier(node.expression.expression) &&
    node.expression.expression.text === "test" &&
    node.expression.name.text === "describe"
  );
}

function isBeforeHookCall(node) {
  return (
    ts.isCallExpression(node) &&
    ts.isPropertyAccessExpression(node.expression) &&
    ts.isIdentifier(node.expression.expression) &&
    node.expression.expression.text === "test" &&
    (node.expression.name.text === "beforeEach" || node.expression.name.text === "beforeAll")
  );
}

/** Static text of a title argument, including the literal spans of a template. */
export function staticTitle(node) {
  if (!node) return null;
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
  if (ts.isTemplateExpression(node)) {
    return node.head.text + node.templateSpans.map((span) => span.literal.text).join(" ");
  }
  return null;
}

function containsIdentifier(node, names) {
  let found = false;
  function visit(child) {
    if (found) return;
    if (ts.isIdentifier(child) && names.has(child.text)) {
      found = true;
      return;
    }
    ts.forEachChild(child, visit);
  }
  visit(node);
  return found;
}

/** The `agent:` value of a spawn payload, when it is a literal. */
function spawnAgentLiteral(callNode) {
  for (const argument of callNode.arguments) {
    if (!ts.isObjectLiteralExpression(argument)) continue;
    for (const property of argument.properties) {
      if (!ts.isPropertyAssignment(property)) continue;
      const name = ts.isIdentifier(property.name) || ts.isStringLiteral(property.name)
        ? property.name.text
        : null;
      if (name !== "agent") continue;
      const value = property.initializer;
      if (ts.isStringLiteral(value) || ts.isNoSubstitutionTemplateLiteral(value)) {
        return { literal: true, id: value.text };
      }
      return { literal: false, id: null };
    }
  }
  return { literal: false, id: null };
}

/**
 * A spawn reaches a real provider CLI when it goes through the spawnAgent
 * fixture, or POSTs /agents/spawn with an agent id that is a real provider (a
 * literal bogus id such as "definitely-no-such-provider" is rejected by the
 * daemon and cannot consume credit).
 */
export function spawnsRealProvider(testNode) {
  let found = false;
  function visit(node) {
    if (found) return;
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === SPAWN_FIXTURE) {
      found = true;
      return;
    }
    if (ts.isCallExpression(node)) {
      const mentionsSpawnPath = node.arguments.some(
        (argument) =>
          (ts.isStringLiteral(argument) || ts.isNoSubstitutionTemplateLiteral(argument)) &&
          argument.text.includes(SPAWN_PATH),
      );
      if (mentionsSpawnPath) {
        const agent = spawnAgentLiteral(node);
        if (!agent.literal || SPAWNABLE_PROVIDER_IDS.has(agent.id)) {
          found = true;
          return;
        }
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(testNode);
  return found;
}

/** Scope chain of a test node: enclosing describe callbacks, then the file. */
function scopeChain(testNode, source) {
  const scopes = [];
  let node = testNode.parent;
  while (node) {
    if (isDescribeCall(node)) scopes.push(node);
    node = node.parent;
  }
  scopes.push(source);
  return scopes;
}

/** A stub gate counts when it is inside the test, or in a before-hook of an enclosing scope. */
export function hasStubGate(testNode, source) {
  if (containsIdentifier(testNode, STUB_GATE_IDENTIFIERS)) return true;
  for (const scope of scopeChain(testNode, source)) {
    let gated = false;
    function visit(node) {
      if (gated) return;
      if (isBeforeHookCall(node) && containsIdentifier(node, STUB_GATE_IDENTIFIERS)) {
        gated = true;
        return;
      }
      if (node !== scope && isDescribeCall(node)) return; // a nested describe owns its own hooks
      ts.forEachChild(node, visit);
    }
    visit(scope);
    if (gated) return true;
  }
  return false;
}

export function providerSourceDeclarations(title, testNode, source) {
  const declarations = [];
  if (/@stub/.test(title) || hasStubGate(testNode, source)) declarations.push("stub");
  if (/@live/.test(title)) declarations.push("live");
  return declarations;
}

/**
 * @returns {Array<{line: number, reason: string}>} 1-indexed lines.
 */
export function providerGateFindings(text, fileName = "spec.ts") {
  const source = ts.createSourceFile(fileName, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const findings = [];
  function visit(node) {
    if (isTestCall(node)) {
      const title = staticTitle(node.arguments[0]);
      if (title !== null && spawnsRealProvider(node)) {
        const declarations = providerSourceDeclarations(title, node, source);
        if (declarations.length !== 1) {
          findings.push({
            line: source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1,
            reason: PROVIDER_GATE_REASON,
            declarations,
          });
        }
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return findings;
}
