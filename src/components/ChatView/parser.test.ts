/**
 * Standalone tests for JSONL and terminal output parsers.
 * Run: npx tsx src/components/ChatView/parser.test.ts
 */

import { parseJsonlSession, parseTerminalOutput } from "./parser";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) {
    passed++;
    console.log(`  \u2713 ${name}`);
  } else {
    failed++;
    console.error(`  \u2717 ${name}${detail ? `: ${detail}` : ""}`);
  }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

// ---------------------------------------------------------------------------
// parseJsonlSession
// ---------------------------------------------------------------------------

console.log("\n--- parseJsonlSession ---\n");

// 1. User text message
{
  const input = `{"type":"user","uuid":"u1","timestamp":"2025-01-01T00:00:00Z","message":{"role":"user","content":[{"type":"text","text":"hello world"}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "1. user text: one message");
  eq(msgs[0].type, "user", "1. user text: type is user");
  eq(msgs[0].content, "hello world", "1. user text: content matches");
  eq(msgs[0].timestamp, "2025-01-01T00:00:00Z", "1. user text: timestamp");
}

// 2. User message with image — images now attached to user message, not separate
{
  const input = `{"type":"user","uuid":"u2","timestamp":"2025-01-01T00:00:01Z","message":{"role":"user","content":[{"type":"text","text":"check this image"},{"type":"image","source":{"data":"aWJhc2U2NA==","media_type":"image/png"}}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "2. user+image: one message (image attached)");
  eq(msgs[0].type, "user", "2. user+image: type is user");
  eq(msgs[0].content, "check this image", "2. user+image: text content");
  assert(msgs[0].attachments !== undefined, "2. user+image: has attachments");
  eq(msgs[0].attachments!.length, 1, "2. user+image: one attachment");
  eq(msgs[0].attachments![0].mimeType, "image/png", "2. user+image: attachment mimeType");
}

// 3. Assistant text message
{
  const input = `{"type":"assistant","uuid":"a1","timestamp":"2025-01-01T00:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"I can help with that."}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "3. assistant text: one message");
  eq(msgs[0].type, "assistant", "3. assistant text: type");
  eq(msgs[0].content, "I can help with that.", "3. assistant text: content");
}

// 4. Assistant with tool_use (Write) -- verify toolInput has full content
{
  const entry = {
    type: "assistant",
    uuid: "a2",
    message: {
      role: "assistant",
      content: [{
        type: "tool_use",
        id: "toolu_w1",
        name: "Write",
        input: { file_path: "/tmp/test.ts", content: 'console.log("hello world");' },
      }],
    },
  };
  const input = JSON.stringify(entry);
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "4. Write tool_use: one message");
  eq(msgs[0].type, "tool_call", "4. Write tool_use: type");
  eq(msgs[0].toolName, "Write", "4. Write tool_use: toolName");
  eq(msgs[0].toolId, "toolu_w1", "4. Write tool_use: toolId");
  assert(msgs[0].toolInput !== undefined, "4. Write tool_use: toolInput exists");
  eq(msgs[0].toolInput?.file_path, "/tmp/test.ts", "4. Write tool_use: toolInput.file_path");
  eq(msgs[0].toolInput?.content, 'console.log("hello world");', "4. Write tool_use: toolInput.content preserved");
  eq(msgs[0].toolArgs, "/tmp/test.ts", "4. Write tool_use: toolArgs = file_path via summarizeInput");
  eq(msgs[0].collapsed, true, "4. Write tool_use: collapsed");
}

// 5. Assistant with tool_use (Read) -- verify toolInput.file_path
{
  const input = `{"type":"assistant","uuid":"a3","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_r1","name":"Read","input":{"file_path":"/home/user/code.py"}}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "5. Read tool_use: one message");
  eq(msgs[0].toolName, "Read", "5. Read tool_use: toolName");
  eq(msgs[0].toolInput?.file_path, "/home/user/code.py", "5. Read tool_use: file_path");
  eq(msgs[0].toolArgs, "/home/user/code.py", "5. Read tool_use: toolArgs");
}

// 6. Assistant with tool_use (Bash) -- verify toolInput.command
{
  const input = `{"type":"assistant","uuid":"a4","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_b1","name":"Bash","input":{"command":"git status && git diff --stat"}}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "6. Bash tool_use: one message");
  eq(msgs[0].toolName, "Bash", "6. Bash tool_use: toolName");
  eq(msgs[0].toolInput?.command, "git status && git diff --stat", "6. Bash tool_use: command preserved");
  eq(msgs[0].toolArgs, "git status && git diff --stat", "6. Bash tool_use: toolArgs");
}

// 7. Tool result -- verify parsing and truncation at 500 chars
{
  const longResult = "X".repeat(600);
  const input = `{"type":"assistant","uuid":"a5","message":{"role":"assistant","content":[{"type":"tool_result","tool_use_id":"toolu_b1","content":"${longResult}"}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "7. tool_result: one message");
  eq(msgs[0].type, "tool_result", "7. tool_result: type");
  eq(msgs[0].toolId, "toolu_b1", "7. tool_result: toolId");
  eq(msgs[0].content.length, 503, "7. tool_result: truncated to 500 + '...'");
  assert(msgs[0].content.endsWith("..."), "7. tool_result: ends with ellipsis");
  eq(msgs[0].collapsed, true, "7. tool_result: collapsed");
}

// 7b. Tool result -- short content not truncated
{
  const input = `{"type":"assistant","uuid":"a5b","message":{"role":"assistant","content":[{"type":"tool_result","tool_use_id":"toolu_short","content":"short output"}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs[0].content, "short output", "7b. tool_result short: no truncation");
  assert(!msgs[0].content.endsWith("..."), "7b. tool_result short: no ellipsis");
}

// 8. Thinking block -- verify truncation at 200 chars
{
  const longThinking = "T".repeat(300);
  const input = `{"type":"assistant","uuid":"a6","message":{"role":"assistant","content":[{"type":"thinking","text":"${longThinking}"}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "8. thinking: one message");
  eq(msgs[0].type, "thinking", "8. thinking: type");
  assert(msgs[0].content.length <= 203, "8. thinking: truncated to ~200 + '...'");
  assert(msgs[0].content.endsWith("..."), "8. thinking: ends with ellipsis");
  eq(msgs[0].collapsed, true, "8. thinking: collapsed");
}

// 9. Image block with base64
{
  const input = `{"type":"assistant","uuid":"a7","message":{"role":"assistant","content":[{"type":"image","source":{"data":"cG5nZGF0YQ==","media_type":"image/jpeg"}}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "9. image base64: one message");
  eq(msgs[0].type, "image", "9. image base64: type");
  eq(msgs[0].imageData, "cG5nZGF0YQ==", "9. image base64: imageData");
  eq(msgs[0].imageMimeType, "image/jpeg", "9. image base64: imageMimeType");
  eq(msgs[0].content, "", "9. image base64: content is empty");
}

// 10. Image block with file path
{
  const input = `{"type":"assistant","uuid":"a8","message":{"role":"assistant","content":[{"type":"image","source":{"path":"/tmp/screenshot.png","media_type":"image/png"}}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "10. image file path: one message");
  eq(msgs[0].imageData, "file:///tmp/screenshot.png", "10. image file path: normalized to file://");
}

// 10b. Image block with file:// prefix already
{
  const input = `{"type":"assistant","uuid":"a8b","message":{"role":"assistant","content":[{"type":"image","source":{"path":"file:///tmp/screenshot.png","media_type":"image/png"}}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs[0].imageData, "file:///tmp/screenshot.png", "10b. image file:// already: no double prefix");
}

// 11. Image block with URL
{
  const input = `{"type":"assistant","uuid":"a9","message":{"role":"assistant","content":[{"type":"image","source":{"url":"https://example.com/img.png","media_type":"image/png"}}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "11. image URL: one message");
  eq(msgs[0].imageData, "https://example.com/img.png", "11. image URL: passthrough");
}

// 12. Multiple content blocks in one assistant message
{
  const input = `{"type":"assistant","uuid":"a10","message":{"role":"assistant","content":[{"type":"text","text":"Let me check."},{"type":"tool_use","id":"toolu_m1","name":"Bash","input":{"command":"ls -la"}},{"type":"text","text":"Here are the results."}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 3, "12. multiple blocks: three messages");
  eq(msgs[0].type, "assistant", "12. multiple blocks: first is assistant text");
  eq(msgs[0].content, "Let me check.", "12. multiple blocks: first content");
  eq(msgs[1].type, "tool_call", "12. multiple blocks: second is tool_call");
  eq(msgs[1].toolName, "Bash", "12. multiple blocks: tool name");
  eq(msgs[2].type, "assistant", "12. multiple blocks: third is assistant text");
  eq(msgs[2].content, "Here are the results.", "12. multiple blocks: third content");
}

// 13. Malformed JSON line -- should skip without error
{
  const input = `not valid json\n{"type":"user","uuid":"u3","message":{"role":"user","content":[{"type":"text","text":"valid line"}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "13. malformed line: skipped, one message parsed");
  eq(msgs[0].content, "valid line", "13. malformed line: valid line parsed");
}

// 14. Empty/whitespace content -- should be skipped
{
  const input = `{"type":"user","uuid":"u4","message":{"role":"user","content":[{"type":"text","text":"   "}]}}`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 0, "14. whitespace content: skipped");
}

// 14b. Empty lines in JSONL
{
  const input = `\n  \n{"type":"user","uuid":"u5","message":{"role":"user","content":[{"type":"text","text":"after blanks"}]}}\n\n`;
  const msgs = parseJsonlSession(input);
  eq(msgs.length, 1, "14b. empty lines: filtered out, one message");
  eq(msgs[0].content, "after blanks", "14b. empty lines: correct content");
}

// ---------------------------------------------------------------------------
// parseTerminalOutput
// ---------------------------------------------------------------------------

console.log("\n--- parseTerminalOutput ---\n");

// 15. User marker
{
  const input = "\u276F ls -la";
  const msgs = parseTerminalOutput(input);
  eq(msgs.length, 1, "15. user marker: one message");
  eq(msgs[0].type, "user", "15. user marker: type");
  eq(msgs[0].content, "ls -la", "15. user marker: content");
}

// 16. Assistant marker -- text
{
  const input = "\u23FA Here is my analysis of the code.";
  const msgs = parseTerminalOutput(input);
  eq(msgs.length, 1, "16. assistant marker: one message");
  eq(msgs[0].type, "assistant", "16. assistant marker: type");
  eq(msgs[0].content, "Here is my analysis of the code.", "16. assistant marker: content");
}

// 17. Tool call with args
{
  const input = "\u23FA Write(/tmp/file.ts)\n  file content here";
  const msgs = parseTerminalOutput(input);
  eq(msgs.length, 1, "17. tool call: one message");
  eq(msgs[0].type, "tool_call", "17. tool call: type");
  eq(msgs[0].toolName, "Write", "17. tool call: toolName");
  assert(msgs[0].toolArgs !== undefined, "17. tool call: toolArgs exists");
  assert(msgs[0].toolArgs!.includes("/tmp/file.ts"), "17. tool call: args contain path");
  eq(msgs[0].collapsed, true, "17. tool call: collapsed");
}

// 18. Tool result (sub-item marker)
{
  const input = "\u23BF Output of command\n  some result data";
  const msgs = parseTerminalOutput(input);
  eq(msgs.length, 1, "18. tool result: one message");
  eq(msgs[0].type, "tool_result", "18. tool result: type");
  assert(msgs[0].content.includes("Output of command"), "18. tool result: content includes text");
  eq(msgs[0].collapsed, true, "18. tool result: collapsed");
}

// 19. Status marker with duration
{
  const input = "\u273B Completed in 3s";
  const msgs = parseTerminalOutput(input);
  eq(msgs.length, 1, "19. status: one message");
  eq(msgs[0].type, "status", "19. status: type");
  assert(msgs[0].content.includes("Completed"), "19. status: content");
  eq(msgs[0].duration, "3s", "19. status: duration parsed");
}

// 19b. Status without duration
{
  const input = "\u273B Processing request";
  const msgs = parseTerminalOutput(input);
  eq(msgs.length, 1, "19b. status no duration: one message");
  eq(msgs[0].duration, undefined, "19b. status no duration: duration undefined");
}

// 20. Empty input
{
  const msgs = parseTerminalOutput("");
  eq(msgs.length, 0, "20. empty input: no messages");
  const msgs2 = parseTerminalOutput("   ");
  eq(msgs2.length, 0, "20. whitespace input: no messages");
}

// ---------------------------------------------------------------------------
// Attachment marker safety tests
// ---------------------------------------------------------------------------

console.log("\n--- Attachment marker safety ---\n");

// The marker format is: \x00«file:/path» — NUL + guillemets
// Verify it never matches normal text
const ATTACH_RE = /\x00\u00ABfile:([^\u00BB]+)\u00BB/g;

// 21. Normal text with brackets — no false match
{
  const text = "Check [attached: /tmp/foo.pdf] and [link](url)";
  const matches = text.match(ATTACH_RE);
  assert(matches === null, "21. no false positive on [attached: ...]");
}

// 22. Markdown links — no false match
{
  const text = "See [docs](https://example.com/file.pdf) for details";
  const matches = text.match(ATTACH_RE);
  assert(matches === null, "22. no false positive on markdown links");
}

// 23. Code with brackets — no false match
{
  const text = "const arr = [attached, file, path]; // [attached: value]";
  const matches = text.match(ATTACH_RE);
  assert(matches === null, "23. no false positive on code with brackets");
}

// 24. Guillemets in normal text — no false match (no NUL prefix)
{
  const text = "Use «file:/etc/passwd» to read config";
  const matches = text.match(ATTACH_RE);
  assert(matches === null, "24. no false positive on guillemets without NUL");
}

// 25. Actual marker — matches correctly
{
  const text = "hello \x00\u00ABfile:/tmp/test.pdf\u00BB world";
  const matches = [...text.matchAll(ATTACH_RE)];
  eq(matches.length, 1, "25. real marker: one match");
  eq(matches[0][1], "/tmp/test.pdf", "25. real marker: path extracted");
}

// 26. Multiple markers
{
  const text = "\x00\u00ABfile:/a.png\u00BB text \x00\u00ABfile:/b.pdf\u00BB";
  const matches = [...text.matchAll(ATTACH_RE)];
  eq(matches.length, 2, "26. multiple markers: two matches");
  eq(matches[0][1], "/a.png", "26. multiple markers: first path");
  eq(matches[1][1], "/b.pdf", "26. multiple markers: second path");
}

// 27. Path with spaces
{
  const text = "\x00\u00ABfile:/Users/foo/My Documents/file.pdf\u00BB";
  const matches = [...text.matchAll(ATTACH_RE)];
  eq(matches.length, 1, "27. path with spaces: matches");
  eq(matches[0][1], "/Users/foo/My Documents/file.pdf", "27. path with spaces: full path");
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed > 0 ? 1 : 0);
