import { describe, expect, it } from "vitest";
import type { HelperFacts } from "./helperBlocks";
import {
  admitSuggestion,
  EMPTY_MEMORY,
  isRetired,
  momentRecord,
  parseHelperMemory,
  recordOutcome,
  recordShown,
  RETIREMENT,
  type HelperMemory,
  type HelperSuggestion,
} from "./helperRules";

const FACTS: HelperFacts = { telegramConnected: false, plugins: [{ id: "jira-client", name: "Jira", installed: false }], blueprintsInUse: false };
const PHONE: HelperSuggestion = { momentId: "waiting_on_user_while_away", block: { kind: "guide", sectionId: "phone" }, benefit: "b" };
const JIRA: HelperSuggestion = { momentId: "service_without_plugin", block: { kind: "plugin", pluginId: "jira-client" }, benefit: "b" };

describe("helper rules", () => {
  it("admits one suggestion at a time", () => {
    expect(admitSuggestion(EMPTY_MEMORY, null, PHONE, FACTS)).toEqual({ admit: true });
    expect(admitSuggestion(EMPTY_MEMORY, PHONE, JIRA, FACTS)).toEqual({ admit: false, reason: "busy" });
  });

  it("never suggests a connected, installed or unknown feature", () => {
    expect(admitSuggestion(EMPTY_MEMORY, null, PHONE, { ...FACTS, telegramConnected: true })).toEqual({ admit: false, reason: "in-use" });
    expect(admitSuggestion(EMPTY_MEMORY, null, PHONE, { ...FACTS, telegramConnected: null })).toEqual({ admit: false, reason: "unknown-use" });
    const installed: HelperFacts = { ...FACTS, plugins: [{ id: "jira-client", name: "Jira", installed: true }] };
    expect(admitSuggestion(EMPTY_MEMORY, null, JIRA, installed)).toEqual({ admit: false, reason: "in-use" });
    expect(admitSuggestion(EMPTY_MEMORY, null, { ...JIRA, block: { kind: "plugin", pluginId: "absent" } }, FACTS)).toEqual({ admit: false, reason: "unknown-use" });
  });

  it("shows each suggestion once and counts the showing once", () => {
    const shown = recordShown(EMPTY_MEMORY, PHONE);
    expect(recordShown(shown, PHONE)).toBe(shown);
    expect(momentRecord(shown, PHONE.momentId).shown).toBe(1);
    expect(admitSuggestion(shown, null, PHONE, FACTS)).toEqual({ admit: false, reason: "seen" });
    expect(admitSuggestion(shown, null, { ...PHONE, momentId: "long_task_finished_unattended" }, FACTS)).toEqual({ admit: true });
  });

  it("remembers Don't show again per moment and counts outcomes", () => {
    let memory = recordOutcome(recordShown(EMPTY_MEMORY, PHONE), PHONE, "muted");
    memory = recordOutcome(recordShown(memory, JIRA), JIRA, "accepted");
    expect(momentRecord(memory, PHONE.momentId)).toMatchObject({ dismissed: 1, muted: true });
    expect(momentRecord(memory, JIRA.momentId)).toMatchObject({ accepted: 1, dismissed: 0, muted: false });
    const other = { ...PHONE, block: { kind: "guide", sectionId: "messenger-plugins" } } as HelperSuggestion;
    expect(admitSuggestion(memory, null, other, { ...FACTS, plugins: [{ id: "telegram-client", name: "Telegram", installed: false }] })).toEqual({ admit: false, reason: "muted" });
  });

  it("retires a moment whose suggestions are mostly dismissed", () => {
    let memory: HelperMemory = EMPTY_MEMORY;
    const blocks = ["a", "b", "c"].map((pluginId) => ({ ...JIRA, block: { kind: "plugin", pluginId } }) as HelperSuggestion);
    blocks.forEach((suggestion, index) => {
      memory = recordShown(memory, suggestion);
      memory = recordOutcome(memory, suggestion, index === 0 ? "accepted" : "dismissed");
    });
    const record = momentRecord(memory, JIRA.momentId);
    expect(record.dismissed / record.shown).toBeGreaterThanOrEqual(RETIREMENT.dismissRate);
    expect(isRetired(record)).toBe(true);
    expect(admitSuggestion(memory, null, JIRA, FACTS)).toEqual({ admit: false, reason: "retired" });
    expect(isRetired({ ...record, shown: RETIREMENT.minShown - 1, dismissed: RETIREMENT.minShown - 1 })).toBe(false);
  });

  it("restores persisted memory and drops malformed entries", () => {
    const memory = recordOutcome(recordShown(EMPTY_MEMORY, PHONE), PHONE, "muted");
    expect(parseHelperMemory(JSON.parse(JSON.stringify(memory)))).toEqual(memory);
    expect(parseHelperMemory({ moments: { bogus: {}, service_without_plugin: { shown: -2, seen: [1, "k"], muted: "yes" } } })).toEqual({
      moments: { service_without_plugin: { shown: 0, accepted: 0, dismissed: 0, muted: false, seen: ["k"] } },
    });
    expect(parseHelperMemory(null)).toEqual(EMPTY_MEMORY);
  });
});
