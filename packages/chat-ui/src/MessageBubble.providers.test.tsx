
import React, { act } from "react";

import { MessageBubble } from "./MessageBubble";
import { parseInboundReport } from "./codeterm/InboundReportCard";
import { CROSS_TAB_BUBBLE_TESTID } from "./crossTabBubble";
import { reduceToolEvents } from "@codeterm/chat-core";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import {
  assert,
  assertIncludes,
  assertLink,
  assertToolCard,
  buttons,
  clickButtonContaining,
  clipboardWrites,
  createSuite,
  expandToolCard,
  mermaidSources,
  renderMessages,
  renderTranscript,
  textOf,
  toolCall,
  toolCardFor,
  waitForEffects,
  windowAny,
} from "./MessageBubble.providers.testkit";
import {
  codexStructuredEditMessages,
  providerTranscriptFixtures,
  waitProviderMessages,
} from "./providerTranscriptFixtures.testdata";

const { test, run } = createSuite("MessageBubble provider");

for (const fixture of providerTranscriptFixtures) {
  test(`${fixture.provider} renders normalized provider transcript branches`, async () => {
    mermaidSources.length = 0;
    clipboardWrites.length = 0;

    const { host, root } = await renderTranscript(fixture);
    try {
      assertIncludes(host, fixture.expected.attachmentName, `${fixture.provider} attachment`);
      assertIncludes(host, fixture.expected.systemLabel, `${fixture.provider} system subtype`);
      assertIncludes(host, "Thinking", `${fixture.provider} thinking summary`);
      assert(host.querySelector("table"), `${fixture.provider} should render markdown table`);
      assertLink(host, fixture.expected.linkHref);
      assertIncludes(host, fixture.expected.codeNeedle, `${fixture.provider} fenced code`);
      assert(
        host.querySelector('[data-testid="mock-mermaid"]'),
        `${fixture.provider} should render mocked mermaid output`,
      );
      assert(
        mermaidSources.some((source) => source.includes(fixture.expected.mermaidNeedle)),
        `${fixture.provider} should send mermaid source to renderer`,
      );
      for (const label of fixture.expected.toolLabels) {
        assertToolCard(host, label);
      }
      await expandToolCard(host, fixture.expected.toolLabels[0]);
      assertIncludes(host, fixture.expected.resultNeedle, `${fixture.provider} tool result`);
      if (fixture.expected.diffNeedle) {
        assertIncludes(host, fixture.expected.diffNeedle, `${fixture.provider} structured edit added line`);
      }
      if (fixture.expected.diffRemovedNeedle) {
        assertIncludes(host, fixture.expected.diffRemovedNeedle, `${fixture.provider} structured edit removed line`);
      }

      await clickButtonContaining(host, "Thinking");
      assertIncludes(host, fixture.expected.thinkingNeedle, `${fixture.provider} expanded thinking`);

      await clickButtonContaining(host, "Copy");
      assert(
        clipboardWrites.some((write) => write.includes(fixture.expected.codeNeedle)),
        `${fixture.provider} copy button should write fenced code`,
      );
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  test(`${fixture.provider} mixed transcript structure stays stable`, async () => {
    const { host, root } = await renderTranscript(fixture);
    try {
      const copyButtonCount = buttons(host).filter((button) => (button.textContent ?? "").includes("Copy")).length;
      const toolCardCount = fixture.expected.toolLabels.filter((label) =>
        toolCardFor(host, label) != null,
      ).length;
      const counts = {
        messageFixtures: fixture.messages.length,
        tables: host.querySelectorAll("table").length,
        links: host.querySelectorAll("a").length,
        mermaid: host.querySelectorAll('[data-testid="mock-mermaid"]').length,
        toolCards: toolCardCount,
        copyButtons: copyButtonCount,
        attachmentMentions: textOf(host).includes(fixture.expected.attachmentName) ? 1 : 0,
      };

      assert(counts.messageFixtures >= 6, `${fixture.provider} fixture should remain mixed`);
      assert(counts.tables === 1, `${fixture.provider} should render one markdown table`);
      assert(counts.links >= 1, `${fixture.provider} should render markdown links`);
      assert(counts.mermaid === 1, `${fixture.provider} should render one mermaid block`);
      assert(counts.toolCards === fixture.expected.toolLabels.length, `${fixture.provider} should render all tool cards`);
      assert(counts.copyButtons >= 1, `${fixture.provider} should expose copy interactions`);
      assert(counts.attachmentMentions >= 1, `${fixture.provider} should render attachment context`);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}

test("wait batches are accessible, lossless, and provider-neutral", async () => {
  for (const [provider, messages] of Object.entries(waitProviderMessages)) {
    const item = reduceToolEvents(messages)[0];
    assert(item.kind === "tool" && item.event.waitBatch, `${provider} normalized casing reaches the wait predicate`);
  }
  const messages: NormalizedChatMessage[] = [
    { id: "wait-one", type: "tool_call", content: "", toolId: "one", toolName: "wait", toolKind: "generic", toolSummary: "first wait", toolResult: "first result", timestamp: "1000", sourceSessionId: "wait-ui" },
    { id: "wait-two", type: "tool_call", content: "", toolId: "two", toolName: "wait_for", toolKind: "generic", toolSummary: "second wait", toolResult: "second result", timestamp: "2500", sourceSessionId: "wait-ui" },
    { id: "command", type: "tool_call", content: "", toolId: "command", toolName: "wait", toolKind: "command", toolSummary: "command must remain separate" },
    { id: "assistant-break", type: "assistant", content: "breaks the run" },
    { id: "wait-three", type: "tool_call", content: "", toolId: "three", toolName: "tool_wait", toolKind: "generic", toolSummary: "third wait" },
  ];
  const items = reduceToolEvents(messages);
  assert(items.length === 4, "interleaved command and assistant preserve timeline order");
  const { host, root } = await renderMessages(messages);
  try {
    const card = host.querySelector<HTMLElement>('[data-testid="chat-wait-batch"]');
    assert(card, "wait run renders a compact card");
    const button = card.querySelector<HTMLButtonElement>('button[aria-label="Expand 2 wait events"]');
    assert(button, "wait batch uses a named real button");
    assert(button.getAttribute("aria-expanded") === "false", "details start collapsed");
    const controls = button.getAttribute("aria-controls");
    assert(controls && host.querySelector(`#${controls}`)?.hasAttribute("hidden"), "aria-controls references the collapsed details region");
    assertIncludes(card, "2 wait events", "visible count");
    assertIncludes(card, "1.5s", "truthful elapsed summary");
    await act(async () => button.dispatchEvent(new windowAny.KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    await waitForEffects();
    assert(button.getAttribute("aria-expanded") === "true", "keyboard/click activation expands details");
    const region = host.querySelector<HTMLElement>(`#${controls}`);
    assert(region?.getAttribute("role") === "region", "details have an accessible region");
    assertIncludes(region ?? host, "first wait", "first member detail remains visible");
    assertIncludes(region ?? host, "first result", "first member result remains visible");
    assertIncludes(region ?? host, "second wait", "second member detail remains visible");
    assert(!textOf(card).includes("command must remain separate"), "unrelated command does not leak into wait details");
    await act(async () => button.dispatchEvent(new windowAny.KeyboardEvent("keydown", { key: " ", bubbles: true })));
    await waitForEffects();
    assert(button.getAttribute("aria-expanded") === "false", "space activation collapses details");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("Claude compact summary renders as collapsed neutral system artifact", async () => {
  const compactBody = [
    "This session is being continued from a previous conversation.",
    "Summary preview line.",
    "Full compact body visible only after expansion.",
  ].join("\n");
  const { host, root } = await renderMessages([
    {
      id: "compact-summary",
      type: "system",
      systemSubtype: "compact_summary",
      content: compactBody,
      collapsed: true,
    },
    {
      id: "compact-boundary",
      type: "status",
      systemSubtype: "compact_boundary",
      content: "Context compacted",
    },
  ]);
  try {
    assertIncludes(host, "Session continued from summary", "compact summary label");
    assertIncludes(host, "Context compacted", "compact boundary status");
    assert(
      !textOf(host).includes("Full compact body visible only after expansion."),
      "compact summary body should be collapsed by default",
    );

    await clickButtonContaining(host, "Session continued from summary");
    assertIncludes(host, "Full compact body visible only after expansion.", "expanded compact summary body");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("Claude system-shaped user rows render neutral after normalization", async () => {
  const { host, root } = await renderMessages([
    {
      id: "dante-compact-summary",
      type: "system",
      systemSubtype: "compact_summary",
      content: [
        "This session is being continued from a previous conversation that ran out of context.",
        "",
        "Summary:",
        "- I (Dante, Opus) was made the CodeTerm orchestrator.",
      ].join("\n"),
      collapsed: true,
      isCompactSummary: true,
      isVisibleInTranscriptOnly: true,
    },
    {
      id: "dante-task-notification",
      type: "status",
      systemSubtype: "task_notification",
      content: "Task completed",
    },
  ]);
  try {
    assertIncludes(host, "Session continued from summary", "Dante compact summary label");
    assertIncludes(host, "Task completed", "Dante task notification status");
    assert(!textOf(host).includes("<task-notification>"), "raw task notification XML should not render");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("Codex injected transcript rows render as collapsed neutral artifacts", async () => {
  const agentInstructions = [
    "# AGENTS.md instructions for /Users/user/Developer/codeterm-alpha",
    "",
    "<INSTRUCTIONS>",
    "Full instructions visible only after expansion.",
    "</INSTRUCTIONS>",
  ].join("\n");
  const { host, root } = await renderMessages([
    {
      id: "codex-agent-instructions",
      provider: "codex",
      type: "system",
      systemSubtype: "agent_instructions",
      content: agentInstructions,
      collapsed: true,
    },
    {
      id: "codex-turn-aborted",
      provider: "codex",
      type: "system",
      systemSubtype: "turn_aborted",
      content: "<turn_aborted>\nThe user interrupted the previous turn on purpose.\n</turn_aborted>",
      collapsed: true,
    },
    {
      id: "codex-environment-context",
      provider: "codex",
      type: "system",
      systemSubtype: "environment_context",
      content: "<environment_context>\n  <cwd>D:/Developer/codeterm</cwd>\n</environment_context>",
      collapsed: true,
    },
  ]);
  try {
    assertIncludes(host, "System context", "Codex instructions label");
    assertIncludes(host, "Environment context", "Codex environment label");
    assertIncludes(host, "Turn interrupted", "Codex turn aborted label");
    assert(
      !textOf(host).includes("Full instructions visible only after expansion."),
      "Codex instructions should be collapsed by default",
    );

    await clickButtonContaining(host, "System context");
    assertIncludes(host, "Full instructions visible only after expansion.", "expanded Codex instructions body");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("codex structured apply_patch renders diff from real-session patch text", async () => {
  const { host, root } = await renderMessages(codexStructuredEditMessages);
  try {
    assertIncludes(host, "edit", "structured edit tool label");
    assertIncludes(host, "mem/mod.rs", "structured edit file path");
    assertIncludes(host, "let body = text.trim()", "structured edit added hunk");
    assertIncludes(host, "val.to_string()", "structured edit removed hunk");
    assertIncludes(host, "+", "structured edit shows added line count");
    assertIncludes(host, "-", "structured edit shows removed line count");
    assert(!textOf(host).includes("Success. Updated the following files"), "structured edit hides redundant result output");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("completed text output stays hidden while collapsed", async () => {
  const { host, root } = await renderMessages([{
      id: "compact-result",
      provider: "codex",
      type: "tool_call",
      content: "",
      toolName: "shell_command",
      toolId: "compact-result-call",
      toolInput: { command: "printf output" },
      toolResult: "first line\n" + "x".repeat(400),
      toolKind: "command",
      toolSummary: "printf output",
      toolDetail: "printf output",
    }]);
  try {
    assert(!host.querySelector('[title="Show full output"]'), "collapsed tool card has no result subtitle");
    assert(!textOf(host).includes("first line"), "collapsed tool result text stays hidden");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("codex exec_command rg search does not render as structured edit diff", async () => {
  const { host, root } = await renderMessages([
    codexStructuredEditMessages[0],
  ]);
  try {
    assert(host.querySelector('[data-testid="chat-tool-call"]'), "exec_command should render search card");
    assertIncludes(host, "Search", "rg search card label");
    assertIncludes(host, "mem_post_request", "rg search query renders");
    assert(!textOf(host).includes("rg -n"), "collapsed search hides raw command");
    assert(!textOf(host).includes("exec_command"), "raw exec_command label should not appear");
    assert(!textOf(host).includes("let body = text.trim()"), "rg search must not show patch diff content");
    const diffMarkers = textOf(host).match(/\+\d+/g) ?? [];
    assert(diffMarkers.length === 0, "rg search must not show +/- edit counts");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("semantic edit diffs auto-expand independently of provider label", async () => {
  const { host, root } = await renderMessages([
    toolCall("semantic-edit", "edit", {
      provider: "provider-one",
      toolEdits: [{ path: "src/primary.ts", old: "before", new: "after" }],
    }),
  ]);
  try {
    const card = host.querySelector<HTMLElement>('[data-testid="chat-tool-call"]');
    assert(card, "expected semantic edit card");
    assert(card.querySelector('[aria-label="Collapse tool details"]'), "semantic edit should start expanded");
    assert(card.querySelector('[data-testid="toolcard-edit-file"]'), "semantic edit should show its visible diff");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("commands, raw input, failures, and agent reports start collapsed", async () => {
  const { host, root } = await renderMessages([
    toolCall("collapsed-command", "command", { toolInput: { command: "npm test" } }),
    toolCall("collapsed-raw-input", "generic", { toolInput: { content: "raw operational input" } }),
    toolCall("collapsed-failed-command", "command", { toolError: true, toolResult: "command failed" }),
    toolCall("collapsed-agent-report", "codeterm", { toolDetail: "agent report STATUS: done" }),
  ], "collapsed-failed-command");
  try {
    const cards = Array.from(host.querySelectorAll<HTMLElement>('[data-testid="chat-tool-call"]'));
    assert(cards.length === 4, "expected each operational tool card");
    assert(cards[0]?.querySelector('[aria-label="Show full command"]'), "command should start collapsed behind a named command disclosure");
    assert(cards[1]?.querySelector('[aria-label="Expand tool details"]'), "raw operational input should start collapsed");
    assert(cards[2]?.querySelector('[aria-label="Show full command"]'), "failed command should start collapsed behind a named command disclosure");
    assert(cards[3]?.querySelector('[aria-label="Show full command"]'), "CodeTerm command should start collapsed behind a named command disclosure");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("explicit expansion choices persist across rerenders", async () => {
  const message: NormalizedChatMessage = {
    id: "persistent-semantic-edit",
    provider: "provider-one",
    type: "tool_call",
    content: "",
    toolName: "apply_change",
    toolId: "persistent-semantic-edit-call",
    toolKind: "edit",
    toolSummary: "src/persist.ts",
    toolEdits: [{ path: "src/persist.ts", old: "before", new: "after" }],
  };
  const { host, root } = await renderMessages([message]);
  try {
    const collapse = host.querySelector<HTMLButtonElement>('[aria-label="Collapse tool details"]');
    assert(collapse, "semantic edit should start expanded");
    await act(async () => {
      collapse.dispatchEvent(new windowAny.MouseEvent("click", { bubbles: true }));
    });
    await waitForEffects();
    assert(host.querySelector('[aria-label="Expand tool details"]'), "explicit collapse should update the card");

    await act(async () => {
      root.render(
        <div>
          {reduceToolEvents([{ ...message, provider: "provider-renamed" }]).map((item) => {
            const rendered = item.kind === "message" ? item.message : item.event.message;
            return <MessageBubble key={rendered.id} message={rendered} item={item.kind === "tool" ? item : undefined} />;
          })}
        </div>,
      );
    });
    await waitForEffects();
    assert(host.querySelector('[aria-label="Expand tool details"]'), "explicit collapse should survive a provider-label rerender");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

void run();
