import { ChatBottomBar, ChatComposerMock } from "../../ds/ChatComposer";
import { ComposerContextHeader, type ComposerContextHeaderProps, type ContextBubble, type ContextKind, type ContextTitleSpec } from "../../ds/ComposerContextHeader";
import { defineSpecimen } from "../types";

const plan: ContextTitleSpec = { kind: "plan", prefix: "Plan:", primary: "Gallery refresh", openable: true };

const blueprints: ContextBubble[] = [
  { id: "release", title: "Nightly release train", meta: "active", tone: "active", dismissible: true },
  { id: "docs", title: "Docs sync", meta: "scheduled", tone: "scheduled", dismissible: true },
  { id: "migration", title: "Legacy migration", meta: "stopped", tone: "stopped" },
];

const crons: ContextBubble[] = [
  { id: "digest", title: "Morning digest", meta: "scheduled", tone: "scheduled", dismissible: true },
  { id: "triage", title: "Inbox triage", meta: "waiting on you", tone: "waiting", dismissible: true },
];

const delayed: ContextBubble[] = [
  { id: "ping", title: "Check whether the release build finished and post the summary", meta: "in 12m", tone: "scheduled" },
  { id: "later", title: "Draft the changelog once QA signs off", tone: "stashed" },
  { id: "standup", title: "Standup notes", meta: "in 2h", tone: "scheduled" },
];

const linked: Partial<Record<ContextKind, ContextBubble[]>> = { blueprints, crons, delayed };

function ComposerContextSpecimen({ narrow = false, ...props }: ComposerContextHeaderProps & { narrow?: boolean }) {
  return <div className={narrow ? "gallery-specimen-fill gallery-specimen-fill--narrow" : "gallery-specimen-fill"}>
    <ChatBottomBar>
      <ComposerContextHeader {...props} />
      <ChatComposerMock />
    </ChatBottomBar>
  </div>;
}

export default defineSpecimen({
  id: "composer-context-header",
  title: "Composer context header",
  stage: "wide",
  category: "Composer",
  where: "src/components/Terminal/ComposerContextHeader.tsx",
  notes: "One line of context above the composer: the plan or task title on the left, one round button per linked kind on the right with a count and an amber alert dot when something waits on you. Each kind unfolds its own tinted bubble strip with a faint pattern: grid for blueprints, a dot field for crons, diagonals for delayed messages. Stashed messages wait behind an infinity mark until scheduled.",
  states: [
    { id: "plan", label: "Plan only", props: { title: plan } },
    { id: "task", label: "Task title", props: { title: { kind: "task", prefix: "Task:", primary: "Mirror composer context header", secondary: "Plan: Gallery refresh", openable: true } } },
    { id: "kinds", label: "Plan + three kinds · closed", props: { title: { ...plan, extra: 2 }, linked } },
    { id: "blueprints", label: "Blueprints open", props: { title: plan, linked, open: "blueprints" } },
    { id: "crons", label: "Crons open · one waiting on you", props: { title: plan, linked, open: "crons" } },
    { id: "delayed", label: "Delayed open · stashed message", props: { title: { kind: "delayed", prefix: "Next:", primary: "in 12m", secondary: delayed[0].title }, linked: { delayed }, open: "delayed" } },
    { id: "narrow", label: "Narrow width", props: { title: { kind: "task", prefix: "Task:", primary: "Mirror composer context header", secondary: "Plan: Gallery refresh" }, linked, open: "delayed", narrow: true } },
  ],
  Component: ComposerContextSpecimen,
});
