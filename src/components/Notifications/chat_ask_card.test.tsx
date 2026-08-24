// Run: npm run test:unit -- chat_ask_card
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { Window } from "happy-dom";
import { SentMessageBubble, sentInvocation } from "@codeterm/chat-ui";
import type { Notification } from "../../lib/types";
import { askAccessibleName, choiceAccessibleName, levelTone } from "../../lib/notifyLevels";
import { reducer } from "../../store/multiplexer/reducer";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatAskCard } from "./ChatAskCard";
import { NotificationPanel } from "./NotificationPanel";

let checked = 0;
function assert(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

const window = new Window({ url: "http://localhost/chat-ask-card" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "PointerEvent", { value: window.PointerEvent, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ASKING_TAB = "tab-orchestrator";
const FIRST_CHOICE = "Delete them";
const SECOND_CHOICE = "Keep, mark unproven";

function ask(overrides: Partial<Notification> = {}): Notification {
  return {
    id: "ask-1",
    tab_id: ASKING_TAB,
    workspace_id: "ws-1",
    title: "Delete the stale win32 baselines?",
    body: "Left in place, the next run passes against baselines nobody reviewed.",
    severity: "Warning",
    timestamp: 1_700_000_000_000,
    read: false,
    actions: [
      { type: "answer", label: FIRST_CHOICE },
      { type: "answer", label: SECOND_CHOICE },
    ],
    answer: { asked_from_tab: ASKING_TAB, answered: null, delivered: false },
    ...overrides,
  };
}

function emptyState(): ExtendedState {
  return {
    workspaces: [],
    tabs: {},
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    pendingCloses: [],
  };
}

const resolveTabName = (ref: string) => (ref === ASKING_TAB ? "Orchestrator" : undefined);

function cardStyle(html: string): string {
  const tag = html.match(/<div data-testid="chat-ask-card"[^>]*>/)?.[0] ?? "";
  return tag.match(/style="([^"]*)"/)?.[1] ?? "";
}

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);

async function renderCard(
  notification: Notification,
  onAnswer: (id: string, label: string) => void,
): Promise<HTMLElement> {
  await act(async () => {
    root.render(
      <ChatAskCard
        notification={notification}
        onAnswer={onAnswer}
        resolveTabName={resolveTabName}
      />,
    );
  });
  return host;
}

async function renderPanel(
  notification: Notification,
  onAnswer: (id: string, label: string) => void,
): Promise<HTMLElement> {
  await act(async () => {
    root.render(
      <NotificationPanel
        notifications={[notification]}
        onMarkRead={() => {}}
        onMarkAllRead={() => {}}
        onDelete={() => {}}
        onClearAll={() => {}}
        onJumpToTab={() => {}}
        onAnswer={onAnswer}
        onClose={() => {}}
      />,
    );
  });
  return host;
}

function choiceButtons(scope: HTMLElement): HTMLElement[] {
  return Array.from(scope.querySelectorAll("button")).filter((button) =>
    [FIRST_CHOICE, SECOND_CHOICE].includes(button.textContent ?? ""),
  ) as HTMLElement[];
}

async function run() {
  // 1. Every level paints from its own existing token. The card carries the
  //    level it resolved, and the colour formula is a color-mix over that token
  //    and the surface — never a literal that would freeze one theme in place.
  const levels: Array<[Notification["severity"], string, string]> = [
    ["Info", "verbose", "--ct-muted-fg"],
    ["Warning", "action_needed", "--ct-yellow"],
    ["Error", "failure", "--ct-red"],
  ];
  for (const [severity, level, token] of levels) {
    const html = renderToStaticMarkup(
      <ChatAskCard notification={ask({ severity })} onAnswer={() => {}} />,
    );
    const style = cardStyle(html);
    assert(
      html.includes(`data-notification-level="${level}"`),
      `${severity} should render as the ${level} level`,
    );
    assert(levelTone(level) === `var(${token})`, `${level} should tone from ${token}`);
    assert(
      style.includes("background:color-mix(") && style.includes(`var(${token})`),
      `${level} background should be a color-mix over ${token}, got: ${style}`,
    );
    assert(
      style.includes("border-color:color-mix(") && style.includes("transparent"),
      `${level} border should be a color-mix over its level token, got: ${style}`,
    );
    assert(
      style.includes("var(--ct-surface)"),
      `${level} background should mix over the surface token, got: ${style}`,
    );
    assert(
      !/#[0-9a-fA-F]{3,8}/.test(style),
      `${level} must not resolve a token to a literal colour, got: ${style}`,
    );
  }

  // 2. Answering in the chat records the answer ON THE NOTIFICATION, and the
  //    panel showing that same record is answered too. One store state, two
  //    surfaces — neither holds an answer of its own.
  let state: ExtendedState = { ...emptyState(), notifications: [ask()] };
  const answerThroughStore = (id: string, label: string) => {
    state = reducer(state, { type: "ANSWER_NOTIFICATION", id, answered: label, delivered: true });
  };
  const stored = () => state.notifications[0];

  let view = await renderCard(stored(), answerThroughStore);
  const offered = choiceButtons(view);
  assert(offered.length === 2, "an unanswered ask should offer one button per choice");
  await act(async () => offered[0].click());
  assert(
    stored().answer?.answered === FIRST_CHOICE,
    "the chosen label should be recorded on the notification",
  );

  view = await renderCard(stored(), answerThroughStore);
  assert(
    view.querySelector('[data-testid="chat-ask-answered"]'),
    "the chat card should read its answered state back off the notification",
  );
  assert(choiceButtons(view).length === 0, "an answered chat card should offer no choices");

  let panel = await renderPanel(stored(), answerThroughStore);
  assert(
    panel.querySelector('[data-testid="notification-answered"]'),
    "the panel card for the same notification should be answered",
  );
  assert(choiceButtons(panel).length === 0, "the answered panel card should offer no choices");

  // 3. A second answer from the other surface is a no-op: the panel offers no
  //    control at all, and the store refuses the label even when one is forced.
  answerThroughStore("ask-1", SECOND_CHOICE);
  assert(
    stored().answer?.answered === FIRST_CHOICE,
    "a second answer must not overwrite the standing one",
  );
  panel = await renderPanel(stored(), answerThroughStore);
  assert(
    panel.textContent?.includes(FIRST_CHOICE) && !panel.textContent?.includes(SECOND_CHOICE),
    "both surfaces should still show the first answer only",
  );

  // 4. The answer is a routed message, so it renders through the existing sent
  //    bubble — the same markup that component produces on its own.
  const answeredAsk = stored();
  view = await renderCard(answeredAsk, answerThroughStore);
  assert(
    view.querySelector('[data-testid="chat-sent-header"]'),
    "the answer should render with the shared sent-message header",
  );
  assert(
    view.querySelector('[data-testid="chat-sent-message"]'),
    "the answer should render inside the shared sent-message bubble",
  );
  const direct = renderToStaticMarkup(
    <SentMessageBubble
      inv={sentInvocation(ASKING_TAB, FIRST_CHOICE)}
      resolveTabName={resolveTabName}
    />,
  );
  const withinCard = renderToStaticMarkup(
    <ChatAskCard
      notification={answeredAsk}
      onAnswer={() => {}}
      resolveTabName={resolveTabName}
    />,
  );
  assert(
    withinCard.includes(direct),
    "the transcript answer should be the shared bubble verbatim, not a local copy",
  );

  // 5. Accessible names are composed from the level and the label the question
  //    carries, so they move when either does.
  view = await renderCard(ask(), answerThroughStore);
  const card = view.querySelector('[data-testid="chat-ask-card"]') as HTMLElement;
  // Read the name out now: React reuses the node across the next render.
  const actionNeededName = card.getAttribute("aria-label");
  assert(
    actionNeededName === askAccessibleName("action_needed", ask().title),
    `the card name should derive from level and label, got: ${actionNeededName}`,
  );
  assert(
    choiceButtons(view)[0].getAttribute("aria-label")
      === choiceAccessibleName("action_needed", ask().title, FIRST_CHOICE),
    "a choice name should place the label inside its question",
  );
  const failing = await renderCard(ask({ severity: "Error" }), answerThroughStore);
  const failingCard = failing.querySelector('[data-testid="chat-ask-card"]') as HTMLElement;
  assert(
    failingCard.getAttribute("aria-label") === askAccessibleName("failure", ask().title),
    "the same question at another level should get another name",
  );
  assert(
    failingCard.getAttribute("aria-label") !== actionNeededName,
    "an accessible name derived from the level cannot be a constant",
  );

  // 6. A record naming a level renders that level, not the one severity alone
  //    would imply (contrast with case 1's `severity: "Info"` → "verbose").
  const leveled = ask({ severity: "Info", level: "failure" });
  const leveledView = await renderCard(leveled, answerThroughStore);
  const leveledCard = leveledView.querySelector('[data-testid="chat-ask-card"]') as HTMLElement;
  assert(
    leveledCard.dataset.notificationLevel === "failure",
    `an explicit level must win over the severity-derived one, got: ${leveledCard.dataset.notificationLevel}`,
  );
  assert(
    leveledCard.getAttribute("aria-label") === askAccessibleName("failure", leveled.title),
    "the accessible name must derive from the declared level, not severity",
  );
  assert(
    levelTone("failure") !== levelTone("verbose"),
    "the fixture must actually distinguish the two tones for this assertion to mean anything",
  );

  await act(async () => root.unmount());
  console.log(`chat_ask_card tests: ${checked} assertions passed`);
}

void run();
