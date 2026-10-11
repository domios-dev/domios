import { describe, expect, it } from "vitest";
import type { ChatMessageKind } from "@codeterm/chat-core";
import { messageRowTime } from "./messageRowTime";
import { FIRST_TIME, timeFixtures } from "./messageTimeFixtures.testdata";

describe("messageRowTime", () => {
  it.each(timeFixtures)("selects the event time for $name", ({ message, item, origin, side }) => {
    expect(messageRowTime(message, item, origin)).toEqual({ timestamp: Date.parse(FIRST_TIME), side });
  });
  it("covers every transcript kind, excluding nonvisual metadata", () => {
    const kinds: Record<ChatMessageKind, boolean> = {
      user: true, assistant: true, tool_call: true, tool_result: true, thinking: true, status: true,
      system: true, local_command: true, image: true, shell_command: true, context_request: true,
      watcher_verdict: true, custom: true, tool_flow: false,
    };
    for (const [type, rendered] of Object.entries(kinds)) {
      if (rendered) expect(timeFixtures.some(({ message }) => message.type === type)).toBe(true);
      else expect(messageRowTime({ id: type, type: type as ChatMessageKind, content: "", timestamp: FIRST_TIME })).toBeNull();
    }
    expect(messageRowTime({ id: "intent", type: "custom", content: "", timestamp: FIRST_TIME, sessionIntent: { title: "fixture" } })).toBeNull();
  });
  it.each([undefined, "", "invalid", "Infinity", "8640000000000001"])("does not invent a time for %s", (timestamp) => {
    expect(messageRowTime({ id: "missing", type: "assistant", content: "fixture", timestamp })).toBeNull();
  });
  it("does not replace an unstamped first grouped event with the last event's time", () => {
    const fixture = timeFixtures.find(({ name }) => name === "command batch")!;
    if (fixture.item?.kind !== "tool") throw new Error("missing group");
    const first = fixture.item.event.commandBatch![0];
    const item = { ...fixture.item, event: { ...fixture.item.event, commandBatch: [{ ...first, message: { ...first.message, timestamp: undefined } }, ...fixture.item.event.commandBatch!.slice(1)] } };
    expect(messageRowTime(fixture.message, item)).toBeNull();
  });
});
