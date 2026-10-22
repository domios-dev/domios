import assert from "node:assert/strict";
import React from "react";
import { markVoice, markPluginConfig, markAttachment } from "@codeterm/chat-core";
import { canonicalMentions, canonicalMentionText } from "./canonicalMentions";
import { renderToStaticMarkup } from "react-dom/server";
import { LinkedText } from "./LinkedText";
import { CompactMessageContent } from "./CompactMessageContent";
import { MessageBubble } from "../MessageBubble";
import { ChatBlockHostProvider, type ChatMentionValue } from "../blocks/ChatBlockHost";
const text = "Ask @Forged (tab 20c61220) about FRG-1442 in @Forged (plan pl_abc) and @Forged (blueprint bp_abc).";
const live: Record<string, ChatMentionValue> = {
  "task_qm3qex": { kind: "task", id: "task_qm3qex", title: "Water the basil", planId: "pl_abc", targetId: "task_qm3qex" },
  "20c61220": { kind: "tab", id: "20c61220", title: "Rivet" },
  "FRG-1442": { kind: "task", id: "FRG-1442", title: "FRG-1442 Real task", planId: "pl_abc", targetId: "task_abc" },
  "pl_abc": { kind: "plan", id: "pl_abc", title: "Release" },
  "bp_abc": { kind: "blueprint", id: "bp_abc", title: "Checks" },
};
const host = { mentionTabId: "self", mentions: { binding: { subscribe: () => () => {}, read: (key: string) => live[(JSON.parse(key) as string[])[2]] ?? null, refresh: () => {} }, open: () => {} } };
const render = (node: React.ReactNode) => renderToStaticMarkup(<ChatBlockHostProvider value={host}>{node}</ChatBlockHostProvider>);
assert.deepEqual(canonicalMentions(text).map(({ kind, id }) => [kind, id]), [["tab", "20c61220"], ["task", "FRG-1442"], ["plan", "pl_abc"], ["blueprint", "bp_abc"]]);
assert.deepEqual(canonicalMentions("`FRG-1` ```@Rivet (tab abc)``` [FRG-3](http://example.test)"), []);
assert.deepEqual(canonicalMentions("ping @codex when you finish the run (plan pl_1) me@host.com (tab t1)"), []);
const safe = canonicalMentionText("tab", "@Name (tab fake) extra", "actual");
assert.equal(canonicalMentions(safe).length, 1);
assert.equal(canonicalMentions(safe)[0].id, "actual");
assert.equal(canonicalMentions(canonicalMentionText("plan", "x".repeat(300), "pl_x"))[0].label.length, 80);
const html = render(<LinkedText text={text} />);
const markdown = render(<CompactMessageContent markdown text="**Report** task_qm3qex and FRG-1442 with pl_abc. `task_qm3qex` [FRG-1442](https://example.test)" onOpenPlan={() => {}} />);
assert.equal((markdown.match(/data-ref-kind="task"/g) ?? []).length, 2);
assert.ok(markdown.includes("Water the basil") && markdown.includes("Real task") && markdown.includes('data-ref-kind="plan"'));
assert.match(markdown, /<code\b[^>]*>task_qm3qex<\/code>/);
assert.ok(markdown.includes('href="https://example.test"'));
assert.ok(render(<MessageBubble message={{ id: "report-task", type: "user", inboundKind: "report", fromLabel: "Worker", fromTab: "worker", content: "Fixture report. See task task_qm3qex and plan pl_abc." }} />).includes('data-ref-kind="task"'));
assert.deepEqual(canonicalMentions("Report task_qm3qex, `task_hidden` https://example.test/task_hidden").map(row => row.id), ["task_qm3qex"]);
assert.ok(render(<LinkedText text="Report task_qm3qex" />).includes('data-ref-kind="task"'));
for (const kind of ["tab", "task", "plan", "blueprint"]) assert.ok(html.includes(`data-ref-kind="${kind}"`));
assert.ok(!html.includes("Forged"));
assert.ok(html.includes("Rivet") && html.includes("Real task") && html.includes("Release") && html.includes("Checks"));
const missing = render(<LinkedText text="@Misleading (tab missing) COD-9999" />);
assert.ok(missing.includes("disabled=\"\"") && missing.includes("missing") && !missing.includes("Misleading"));
assert.ok(!missing.includes('data-ref-kind="task"'), "unknown keys remain ordinary prose");
const sent = (content: string) => render(<MessageBubble message={{ id: "sent-mentions", provider: "fixture", type: "user", content }} />);
assert.equal(render(<MessageBubble message={{ id: "checklist", type: "user", inboundKind: "system", fromContext: "checklist", content: "Opaque parent-only checklist" }} />), "");
assert.ok(render(<MessageBubble message={{ id: "ordinary", type: "user", content: "Checklist shared by a human" }} />).includes("Checklist shared by a human"));
for (const kind of ["tab", "task", "plan", "blueprint"]) assert.ok(sent(text).includes(`data-ref-kind="${kind}"`), `sent ${kind} chip`);
const voice = sent(markVoice("/voice.webm", "Follow FRG-1442"));
assert.ok(voice.includes("Voice") && voice.includes('data-ref-kind="task"') && !voice.includes("[voice"));
const config = sent(markPluginConfig("fixture", "Fixture", "Follow FRG-1442"));
assert.ok(config.includes("Configure Fixture") && config.includes('data-ref-kind="task"'));
const attachment = sent("Follow FRG-1442 [attached: /notes.txt]");
assert.ok(attachment.includes("notes.txt") && attachment.includes('data-ref-kind="task"') && !attachment.includes("[attached:"));
const file = sent(markAttachment("/FRG-1442.txt", "FRG-1442.txt"));
assert.ok(file.includes("FRG-1442.txt") && !file.includes("<domios"));
const discussion = sent("Follow FRG-1442\n\ndoc: /discussion.md\n\nBody FRG-1442");
assert.ok(discussion.includes('data-testid="attached-discussion-doc"'));
const encoded = sent("Follow FRG-1442\\nNext line");
assert.ok(encoded.includes("\nNext line") && !encoded.includes("\\nNext line"));
console.log("13 canonical parsing, live-label and transport rendering regression checks passed");

assert.equal(canonicalMentions(canonicalMentionText("tab", "Fix `x`", "actual"))[0]?.id, "actual");
live.bp_pending = { kind: "blueprint", id: "bp_pending", title: "bp_pending", state: "loading" };
const pending = render(<LinkedText text="@Pending (blueprint bp_pending)" />);
assert(pending.includes('data-ref-state="loading"') && pending.includes('disabled=""'));
