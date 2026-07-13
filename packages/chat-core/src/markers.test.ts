import {
  MARKER_OPEN,
  isPaneMarkerPlaceholderLabel,
  markContextRequest,
  markPluginConfig,
  markSystemPrompt,
  markWatcherVerdict,
  parseMarker,
  shortPaneRef,
} from "./markers";

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
  ` ${MARKER_OPEN}pane label=main pane=bda0e267 model=Opus-=-Payload with space`;
const parsed = parseMarker(leadingSpaceMarker);

eq(parsed?.kind, "pane", "parseMarker strips leading whitespace");
eq(parsed?.attrs, { label: "main", pane: "bda0e267", model: "Opus" }, "leading-space marker attrs");
eq(parsed?.payload, "Payload with space", "leading-space marker payload");

const normal = parseMarker(`${MARKER_OPEN}pane label=clean-=-ok`);
eq(normal?.kind, "pane", "parseMarker keeps no-leading-space case");
eq(normal?.payload, "ok", "parseMarker keeps payload unchanged");

const bracketedPaste = parseMarker(
  `\x1b[200~${MARKER_OPEN}pane label=main pane=bda0e267-=-hello\x1b[201~`,
);
eq(bracketedPaste?.kind, "pane", "parseMarker unwraps bracketed paste start");
eq(bracketedPaste?.payload, "hello", "parseMarker strips bracketed paste end from payload");

eq(isPaneMarkerPlaceholderLabel("CURRENT PANE"), true, "CURRENT PANE is a pane label placeholder");
eq(isPaneMarkerPlaceholderLabel(""), true, "blank pane label is a placeholder");
eq(isPaneMarkerPlaceholderLabel("Puccini"), false, "real pane label is not a placeholder");
eq(shortPaneRef("915fae8effffffff"), "915fae8e", "shortPaneRef keeps display-safe pane id");

// `message_markers_tests.rs` PARITY[6] exactly (name space percent-encoded).
eq(
  markPluginConfig("lmstudio", "LM Studio", "hello body"),
  "-=-codeterm:plugin_config plugin=lmstudio name=LM%20Studio-=-hello body",
  "markPluginConfig wire matches Rust parity fixture",
);

const systemPrompt = "You are CodeTerm.\nUse tools carefully.";
const systemPromptWire = markSystemPrompt(systemPrompt);
eq(
  systemPromptWire,
  "-=-codeterm:system_prompt-=-You are CodeTerm.\nUse tools carefully.",
  "markSystemPrompt wire matches Rust parity fixture",
);
const systemPromptParsed = parseMarker(systemPromptWire);
eq(systemPromptParsed?.kind, "system_prompt", "system_prompt marker kind parses");
eq(systemPromptParsed?.attrs, {}, "system_prompt marker has no attrs");
eq(systemPromptParsed?.payload, systemPrompt, "system_prompt payload is prompt text");

const contextBody = '{"state":{},"input":{"tick":1}}';
const contextWire = markContextRequest(contextBody);
eq(contextWire, "-=-codeterm:context_request-=-{\"state\":{},\"input\":{\"tick\":1}}", "markContextRequest wire");
eq(parseMarker(contextWire)?.kind, "context_request", "context_request marker kind parses");

const verdictBody = '{"status":"ok","summary":"All good"}';
const verdictWire = markWatcherVerdict(verdictBody);
eq(verdictWire, "-=-codeterm:watcher_verdict-=-{\"status\":\"ok\",\"summary\":\"All good\"}", "markWatcherVerdict wire");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
