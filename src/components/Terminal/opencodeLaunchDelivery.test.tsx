import { expect, test } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { tabActivity } from "../../lib/tabActivity";
import { ChatDeliveryState } from "./ChatDeliveryState";
import { deliveryState } from "./chatDeliveryModel";
import { observeDeliveryStatus, readChatSends, beginChatSendRetry, forgetChatSendsInMemory } from "./chatSendStore";
import type { ChatSendStatus } from "./chatSendStatus";


test("a lazy provider's ready composer ends Starting before a session exists", () => {
  const tab = { agent_type: "lazy-fixture", agent_session_id: null, agent_booting: false };
  expect(tabActivity({ ...tab, boot_stage: "launch_written" }).kind).toBe("starting");
  expect(tabActivity({ ...tab, boot_stage: "agent_ready" }).kind).toBe("idle");
  expect(tabActivity({ ...tab, boot_stage: "agent_ready", pending_prompt: { question: "opaque", options: [] } }).kind).toBe("waiting-on-user");
});

test("continuation status hydrates one bubble and never enables retry of uncertain input", () => {
  const status: ChatSendStatus = {
    tab_id: "continue-fixture", send_id: "continue-send", correlationId: "continue-send",
    state: "held", ledger_id: "ledger", sequence: 1, retained_from: 1, error: null,
    system_send: { content: "opaque", sent_at_ms: 1000 },
  };
  observeDeliveryStatus(status);
  observeDeliveryStatus({ ...status, state: "submit_unconfirmed", sequence: 2 });
  forgetChatSendsInMemory();
  const sends = readChatSends(status.tab_id);
  expect(sends).toHaveLength(1);
  expect(sends[0].content).toBe(status.system_send!.content);
  expect(deliveryState(sends[0])).toBe("uncertain");
  expect(beginChatSendRetry(status.tab_id, sends[0].id)).toBeUndefined();
  const host = document.createElement("div");
  host.innerHTML = renderToStaticMarkup(<ChatDeliveryState state={deliveryState(sends[0])} />);
  expect(host.querySelector('[role="status"]')).not.toBeNull();
  expect(host.querySelector("button")).toBeNull();
  observeDeliveryStatus({ ...status, state: "delivered", sequence: 3 });
  expect(readChatSends(status.tab_id)).toHaveLength(1);
  expect(deliveryState(readChatSends(status.tab_id)[0])).toBe("delivered");
});
