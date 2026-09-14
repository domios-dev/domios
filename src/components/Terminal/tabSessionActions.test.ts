import assert from "node:assert/strict";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { permissionAutoConfirmMenuState, tabSessionActionView } from "./tabSessionActions";

function tab(overrides: Partial<Tab>): Tab {
  return { id: "tab", pty_id: null, tab_type: "Terminal", title: "Tab", cwd: null, ...overrides };
}

assert.deepEqual(
  tabSessionActionView(tab({ agent_type: "claude", agent_session_id: "agent-1", backing: { kind: "pty" } })),
  { sessionId: "agent-1", canReconnect: true, canRestart: true },
);
assert.deepEqual(
  tabSessionActionView(tab({ backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "chat-1" } })),
  { sessionId: "chat-1", canReconnect: true, canRestart: false },
);
assert.deepEqual(
  tabSessionActionView(tab({ backing: { kind: "pty" } })),
  { sessionId: null, canReconnect: false, canRestart: false },
);

const recognizer = { supports_permission_auto_confirm: true } as ProviderDescriptor;
const nonRecognizer = { supports_permission_auto_confirm: false } as ProviderDescriptor;
const ptyAgent = (enabled?: boolean) =>
  tab({ agent_type: "agent-a", backing: { kind: "pty" }, permission_auto_confirm: enabled });

assert.equal(permissionAutoConfirmMenuState(ptyAgent(true), recognizer), true);
assert.equal(permissionAutoConfirmMenuState(ptyAgent(false), recognizer), false);
assert.equal(permissionAutoConfirmMenuState(ptyAgent(undefined), recognizer), false);
assert.equal(permissionAutoConfirmMenuState(ptyAgent(true), nonRecognizer), null);
assert.equal(permissionAutoConfirmMenuState(ptyAgent(true), undefined), null);
assert.equal(permissionAutoConfirmMenuState(tab({ backing: { kind: "pty" } }), recognizer), null);
assert.equal(
  permissionAutoConfirmMenuState(
    tab({ agent_type: "agent-a", backing: { kind: "chat_backend", plugin_id: "p", session_id: "s" } }),
    recognizer,
  ),
  null,
);

console.log("Tab session action tests: passed");
