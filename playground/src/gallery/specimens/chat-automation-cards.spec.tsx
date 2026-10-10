import type { ChatOrigin, NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "@codeterm/chat-ui";
import { defineSpecimen } from "../types";

type Card = { id: string; content: string; origin: ChatOrigin };

const cron: Card = {
  id: "cron-end",
  content: "Nightly triage finished: **3 new issues** labelled, 1 duplicate closed. Summary is in `~/.codeterm/cron/nightly.md`.",
  origin: { kind: "automation", source: "cron", name: "nightly-triage", detail: "completed" },
};
const cronReport: Card = {
  id: "cron-report",
  content: "Release notes draft is ready for review; two PR titles need rewording.",
  origin: { kind: "automation", source: "cron", name: "release-notes", tabIdShort: "a1b2c3d4", report: true, model: "opus" },
};
const blueprint: Card = {
  id: "blueprint-result",
  content: "Blueprint bp_docs_sync: done. Read the result in `.codeterm/blueprints/bp_docs_sync/result.md`.",
  origin: { kind: "automation", source: "blueprint", name: "bp_docs_sync", detail: "result" },
};
const blueprintFailed: Card = {
  id: "blueprint-failed",
  content: "Blueprint bp_ci_watch stopped: the verify step exited 1 three times.",
  origin: { kind: "automation", source: "blueprint", name: "bp_ci_watch", detail: "lifecycle:failed" },
};
const shell = (purpose: string, name: string, content: string): Card => ({
  id: `shell-${purpose}`,
  content,
  origin: { kind: "automation", source: "utility", name, purpose, tabIdShort: "c0ffee12" },
});
const SHELLS = [
  shell("build", "Builder", "`pnpm build` failed: `src/App.tsx(12,3): TS2304 Cannot find name 'BackPill'`."),
  shell("qa", "Verifier", "Vitest: 303 passed, 1 failed — `MessageBubble.watcher.test.tsx`."),
  shell("dev", "Server", "Dev server is up on http://127.0.0.1:1420."),
  shell("deploy", "Deploy", "Staging deploy finished, version 1.14.0-rc.3."),
  shell("logs", "Logs", "3 warnings in the last minute: `usage.parser.cursor probe failed`."),
  shell("db", "Database", "Migration 0042 applied, 12 rows backfilled."),
];

const noop = () => undefined;

function AutomationStage({ cards }: { cards: Card[] }) {
  return <div className="gallery-fill gallery-transcript">
    {cards.map((card) => {
      const message: NormalizedChatMessage = { id: card.id, type: "user", content: card.content, inboundKind: "system" };
      return <MessageBubble key={card.id} message={message} origin={card.origin} onOriginClick={noop} onOpenJobs={noop} />;
    })}
  </div>;
}

export default defineSpecimen({
  id: "chat-automation-cards",
  title: "Chat automation cards",
  category: "Chat",
  where: "packages/chat-ui/src/codeterm/AutomationCard.tsx · src/lib/chatPrefixes.ts",
  stage: "wide",
  notes: "Cron, blueprint and utility-shell deliveries parsed from their <domios> frames into one colour-coded card: cron amber, blueprint blue, shells toned by purpose (build, server, qa, deploy, logs, db). Cron and blueprint cards open the job; shell and report cards open the sending tab.",
  Component: AutomationStage,
  states: [
    { id: "cron", label: "Cron · end of run + report", props: { cards: [cron, cronReport] } },
    { id: "blueprint", label: "Blueprint · result + lifecycle", props: { cards: [blueprint, blueprintFailed] } },
    { id: "shells", label: "Utility shells · every purpose", props: { cards: SHELLS } },
  ],
});
