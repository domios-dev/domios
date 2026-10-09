import { formatAttachmentMarker, type ChatOrigin, type NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "@codeterm/chat-ui";
import { useEffect, useRef } from "react";
import { configureLocalPreviews } from "../../../../src/lib/configureLocalPreviews";
import { mockCommand } from "../../mocks/backend";
import { defineSpecimen } from "../types";

const MINUTE = 60_000;
const SNIP_PATH = "/tmp/snips/snip-gallery.png";
const SNIP_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="200" viewBox="0 0 320 200"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#1f3b63"/><stop offset="1" stop-color="#0d1524"/></linearGradient></defs><rect width="320" height="200" rx="14" fill="url(#g)"/><rect x="24" y="28" width="150" height="12" rx="6" fill="#8fb4ff" opacity=".8"/><rect x="24" y="54" width="250" height="8" rx="4" fill="#cdd9ee" opacity=".35"/><rect x="24" y="70" width="210" height="8" rx="4" fill="#cdd9ee" opacity=".35"/><rect x="24" y="120" width="92" height="30" rx="15" fill="#5b8cff"/></svg>`;

configureLocalPreviews();
mockCommand("read_local_preview_file", (args) => args?.path === SNIP_PATH
  ? { data: `data:image/svg+xml;base64,${btoa(SNIP_SVG)}`, media_type: "image/svg+xml" }
  : Promise.reject(new Error("not in the gallery fixtures")));

const SNIP = [
  "## Browser snip from the user",
  formatAttachmentMarker(SNIP_PATH, "image"),
  "**Page:** developer.apple.com — `https://developer.apple.com/`",
  "**User says:** what is inside?",
].join("\n");

const SYSTEM_PROMPT = [
  "You are running inside **CodeTerm**, an AI-first terminal multiplexer.",
  "",
  "- Reply to the user in their language; agent-to-agent traffic stays in English.",
  "- Use `codeterm mem search` before answering about people, projects or past events.",
  "- Finished work for a parent goes through `codeterm agent report`.",
].join("\n");

type Card = { id: string; content: string; origin: ChatOrigin; expand?: boolean };

const now = Date.now();
const notice = (id: string, expand = false): Card => ({ id, content: SNIP, origin: { kind: "system", context: "browser snip" }, expand });
const prompt = (id: string, expand = false): Card => ({ id, content: SYSTEM_PROMPT, origin: { kind: "system_prompt" }, expand });
const oneShot = (id: string): Card => ({
  id,
  content: "Check whether the release build finished and post the summary in #release.",
  origin: { kind: "reminder", whoSet: "Release planner", model: "opus", setAt: String(now - 42 * MINUTE), fireAt: String(now - 2 * MINUTE), id: "delayed-one-shot" },
});
const recurring = (id: string): Card => ({
  id,
  content: "Poll the nightly QA run and flag any new failures.",
  origin: { kind: "reminder", whoSet: "QA lead", model: "sonnet", setAt: String(now - 3 * 60 * MINUTE), fireAt: String(now - MINUTE), id: "delayed-recurring", everyMs: String(30 * MINUTE), nextAt: String(now + 29 * MINUTE), skippedTicks: "2" },
});

const noop = () => undefined;

function NoticeStage({ cards }: { cards: Card[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let attempts = 0;
    let timer = 0;
    const expand = () => {
      const heads = cards.flatMap((card) => card.expand
        ? Array.from(ref.current?.querySelectorAll<HTMLButtonElement>(`[data-card-id="${card.id}"] .ct-card__head[aria-expanded="false"]`) ?? [])
        : []);
      for (const head of heads) head.click();
      if (cards.some((card) => card.expand) && !heads.length && attempts++ < 20) timer = window.setTimeout(expand, 50);
    };
    expand();
    return () => window.clearTimeout(timer);
  }, [cards]);
  return <div ref={ref} className="gallery-fill gallery-transcript">
    {cards.map((card) => {
      const message: NormalizedChatMessage = { id: card.id, type: "user", content: card.content, inboundKind: "system" };
      return <div key={card.id} data-card-id={card.id}>
        <MessageBubble message={message} origin={card.origin} onStopReminder={noop} />
      </div>;
    })}
  </div>;
}

export default defineSpecimen({
  id: "chat-notice-cards",
  title: "Chat notice cards",
  category: "Chat",
  where: "packages/chat-ui/src/MessageBubble.tsx · packages/chat-ui/src/ChatNoticeCard.tsx · packages/chat-ui/src/codeterm/ReminderCard.tsx",
  stage: "wide",
  notes: "The shipped MessageBubble routing system notices, the system prompt and reminders onto the shared ct-card surface. Expanded stages open the card through its own header button; the notice's image marker loads through the product preview loader with a mocked read_local_preview_file.",
  Component: NoticeStage,
  states: [
    { id: "notice-collapsed", label: "System notice · collapsed", props: { cards: [notice("notice-collapsed")] } },
    { id: "notice-expanded", label: "System notice · markdown + image marker", props: { cards: [notice("notice-expanded", true)] } },
    { id: "prompt-collapsed", label: "System prompt · collapsed", props: { cards: [prompt("prompt-collapsed")] } },
    { id: "prompt-expanded", label: "System prompt · expanded", props: { cards: [prompt("prompt-expanded", true)] } },
    { id: "reminder-once", label: "Reminder · one-shot", props: { cards: [oneShot("reminder-once")] } },
    { id: "reminder-recurring", label: "Reminder · recurring with Stop", props: { cards: [recurring("reminder-recurring")] } },
    { id: "stack", label: "One transcript · shared ct-card surface", props: { cards: [prompt("stack-prompt"), notice("stack-notice"), oneShot("stack-once"), recurring("stack-recurring")] } },
  ],
});
