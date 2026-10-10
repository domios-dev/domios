import {
  markMesh,
  markReport,
  markTab,
  markTelegram,
  MARKER_OPEN,
  isTabMarkerPlaceholderLabel,
  markContextRequest,
  markPluginConfig,
  markSystemPrompt,
  markWatcherVerdict,
  parseMarker,
  shortTabRef,
} from "./markers";
import {
  buildDomiosMessage,
  buildDomiosOpeningTag,
  DOMIOS_TYPED_INSTRUCTION,
  parseDomiosMessage,
} from "./domios";

let passed = 0,
  failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

const leadingSpaceMarker =
  ' <domios from="tab" label="main" tab="bda0e267" model="Opus">Payload with space</domios>';
eq(parseMarker(leadingSpaceMarker), null, "leading whitespace cannot create route provenance");

const normal = parseMarker('<domios from="tab" label="clean" tab="abc12345">ok</domios>');
eq(normal?.kind, "tab", "parseMarker keeps no-leading-space case");
eq(normal?.payload, "ok", "parseMarker keeps payload unchanged");

const bracketedPaste = parseMarker(
  '\x1b[200~<domios from="tab" label="main" tab="bda0e267">hello</domios>\x1b[201~',
);
eq(bracketedPaste, null, "a pasted Domios frame is content, not a typed route frame");

eq(isTabMarkerPlaceholderLabel("CURRENT Tab"), true, "CURRENT Tab is a tab label placeholder");
eq(isTabMarkerPlaceholderLabel(""), true, "blank tab label is a placeholder");
eq(isTabMarkerPlaceholderLabel("Puccini"), false, "real tab label is not a placeholder");
eq(shortTabRef("915fae8effffffff"), "915fae8e", "shortTabRef keeps display-safe Tab id");

const routeFixtures = [
  [markTelegram("123", "7", "telegram body"), "telegram"],
  [markMesh("peer-a", "abc12345", "mesh body"), "mesh"],
  [markTab("General Main", "bda0e267", "tab body"), "tab"],
  [markReport("Worker", "report body"), "report"],
] as const;
for (const [wire, kind] of routeFixtures) {
  eq(parseDomiosMessage(wire)?.attrs.from, kind, `${kind} route uses a complete Domios frame`);
  eq(parseMarker(wire)?.kind, kind, `${kind} route shares the marker parser`);
  eq(wire.endsWith("</domios>"), true, `${kind} route has the terminal tag`);
}
eq(
  buildDomiosOpeningTag({ from: "tab", label: "first\nsecond" }),
  '<domios from="tab" label="first second">',
  "typed route attributes cannot add a line break",
);
const typedDomios = buildDomiosMessage(
  { from: "tab", tab: "abc12345" },
  "opaque routed body",
  ` [to reply, run: codeterm send "YOUR REPLY" --tab abc12345] ${DOMIOS_TYPED_INSTRUCTION}`,
);
eq(
  parseDomiosMessage(typedDomios)?.body,
  "opaque routed body",
  "typed instruction is stripped from the routed message body",
);
eq(parseMarker("-=-codeterm:tab label=Main tab=abc12345-=-body"), null, "legacy route grammar is rejected");

// `message_markers_tests.rs` PARITY[6] exactly (name space percent-encoded).
eq(
  markPluginConfig("lmstudio", "LM Studio", "hello body"),
  "-=-codeterm:plugin_config plugin=lmstudio name=LM%20Studio-=-\nhello body",
  "markPluginConfig wire matches Rust parity fixture",
);

const systemPrompt = "You are CodeTerm.\nUse tools carefully.";
const systemPromptWire = markSystemPrompt(systemPrompt);
eq(
  systemPromptWire,
  "-=-codeterm:system_prompt-=-\nYou are CodeTerm.\nUse tools carefully.",
  "markSystemPrompt wire matches Rust parity fixture",
);
const systemPromptParsed = parseMarker(systemPromptWire);
eq(systemPromptParsed?.kind, "system_prompt", "system_prompt marker kind parses");
eq(systemPromptParsed?.attrs, {}, "system_prompt marker has no attrs");
eq(systemPromptParsed?.payload, systemPrompt, "system_prompt payload is prompt text");

const contextBody = '{"state":{},"input":{"tick":1}}';
const contextWire = markContextRequest(contextBody);
eq(contextWire, "-=-codeterm:context_request-=-\n{\"state\":{},\"input\":{\"tick\":1}}", "markContextRequest wire");
eq(parseMarker(contextWire)?.kind, "context_request", "context_request marker kind parses");

const verdictBody = '{"status":"ok","summary":"All good"}';
const verdictWire = markWatcherVerdict(verdictBody);
eq(verdictWire, "-=-codeterm:watcher_verdict-=-\n{\"status\":\"ok\",\"summary\":\"All good\"}", "markWatcherVerdict wire");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
