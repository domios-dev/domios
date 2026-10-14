// Run: npm run test:unit -- notificationAnswerDelivery
import assert from "node:assert/strict";
import { createCapabilitiesTransport, type CapabilitiesClient } from "./transport/capabilities";
import { sendNotificationAnswer, type AnswerResult } from "./notificationAnswerSend";
import { reducer } from "../store/multiplexer/reducer";
import type { ExtendedState } from "../store/multiplexer/tree";
import type { Notification } from "./types";
import type { AnswerUpdate } from "./notificationAnswer";

const ASKING_TAB = "tab-asking";
const QUESTION_ID = "ask/42";

function question(): Notification {
  return {
    id: QUESTION_ID, tab_id: ASKING_TAB, workspace_id: null, title: "Grant access?", body: null,
    severity: "Warning", level: "action_needed", timestamp: 1, read: false,
    actions: [{ type: "answer", label: "Grant" }, { type: "answer", label: "Skip" }],
    answer: { asked_from_tab: ASKING_TAB, answered: null, delivered: false },
  };
}

function initial(): ExtendedState {
  return {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [question()], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
  };
}

/** The daemon side of the route: one recording per question, the label carried to the asking tab once. */
function daemon(refuse?: string) {
  const posts: Array<[string, string, unknown]> = [];
  const delivered: Array<{ toTab: string; label: string }> = [];
  let standing: string | null = null;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const json = async <T>(method: string, path: string, body?: unknown): Promise<T> => {
    posts.push([method, path, body]);
    await gate;
    if (refuse) throw new Error(refuse);
    const label = (body as { label: string }).label;
    if (standing === null) {
      standing = label;
      delivered.push({ toTab: ASKING_TAB, label });
    }
    return { answered: standing, delivered: true } as T;
  };
  const client: CapabilitiesClient = { json, localJson: json, currentToken: () => "token" };
  return { posts, delivered, release, transport: createCapabilitiesTransport(client) };
}

async function tapThroughStore(server: ReturnType<typeof daemon>, labels: string[]) {
  let state = initial();
  const pending = new Set<string>();
  const send = (id: string, label: string) => server.transport.answer_notification({ id, label }) as Promise<AnswerResult>;
  const update = (id: string, update: AnswerUpdate) => {
    state = reducer(state, { type: "UPDATE_NOTIFICATION_ANSWER", id, update });
  };
  const taps = labels.map((label) => sendNotificationAnswer(
    QUESTION_ID, label, pending, state.notifications.find((n) => n.id === QUESTION_ID), send, update,
  ));
  assert.equal(state.notifications[0].answer_pending, true, "the store shows the tap as sending before the route answers");
  server.release();
  await Promise.all(taps);
  return state.notifications[0];
}

async function run() {
  const server = daemon();
  const settled = await tapThroughStore(server, ["Grant", "Grant", "Skip"]);
  assert.deepEqual(server.posts, [["POST", `/api/notifications/${encodeURIComponent(QUESTION_ID)}/answer`, { label: "Grant" }]]);
  assert.deepEqual(server.delivered, [{ toTab: ASKING_TAB, label: "Grant" }], "the asking tab receives the label once");
  assert.equal(settled.answer?.answered, "Grant");
  assert.equal(settled.answer?.delivered, true);
  assert.equal(settled.answer_pending, false);

  const refusing = daemon("That choice was not offered by this notification");
  const refused = await tapThroughStore(refusing, ["Grant"]);
  assert.equal(refusing.delivered.length, 0);
  assert.equal(refused.answer?.answered, null);
  assert.match(refused.answer_error ?? "", /not offered/, "a refused answer surfaces as an error on the record");
  assert.equal(refused.answer_pending, false);

  console.log("notification answer delivery: passed");
}

void run();
