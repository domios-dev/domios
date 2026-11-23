import type { ChatOrigin, NormalizedChatMessage } from "@codeterm/chat-core";
import { ago, ahead, MINUTE } from "./clock";

export type ChatCard = { message: NormalizedChatMessage; origin?: ChatOrigin };

const card = (id: string, content: string, origin: ChatOrigin): ChatCard => ({ message: { id, type: "user", content, inboundKind: "system" }, origin });

export const CHAT_CARDS: ChatCard[] = [
  card("cron-soak", "Nightly sync soak finished: **48,000 edits** across 6 replicas, 0 divergent notes. Report in `~/.domios/cron/sync-soak.md`.",
    { kind: "automation", source: "cron", name: "nightly-sync-soak", detail: "completed" }),
  card("bp-release", "Blueprint Release gate 2.4: done. Merged lane/offline-sync-resolver, tests **1,284 passed** on gpu-rig.",
    { kind: "automation", source: "blueprint", name: "release-gate-2.4", detail: "result" }),
  card("qa-sync", "Vitest: 1,284 passed, 0 failed in `packages/sync/` (41.2 s).",
    { kind: "automation", source: "utility", name: "Verifier", purpose: "qa", tabIdShort: "7c1e09ab" }),
  card("reminder-gate", "Check whether the 2.4 release gate went green on gpu-rig and post the summary to the offline sync channel.",
    { kind: "reminder", whoSet: "Helm", model: "opus", setAt: String(ago(25)), fireAt: String(ago(0)), id: "delayed-release-gate" }),
  card("cron-telegram", "Telegram inbox triage: 3 chats answered, 1 needs you: a beta tester asks when offline sync ships.",
    { kind: "automation", source: "cron", name: "telegram-inbox-triage", tabIdShort: "e41f2c90", report: true, model: "sonnet" }),
  card("reminder-digest", "Post the morning standup digest to the owner's Telegram.",
    { kind: "reminder", whoSet: "Ledger", model: "haiku", setAt: String(ago(3 * 60)), fireAt: String(ago(1)), id: "delayed-digest", everyMs: String(60 * MINUTE), nextAt: String(ahead(59)) }),
];

export const CHAT_REPLY: ChatCard = {
  message: { id: "reply-gate", type: "assistant", content: "Release gate 2.4 is **green** on gpu-rig: resolver merged, 1,284 tests passed, sync QA 48 of 48. Posted the summary to the offline sync channel." },
};
