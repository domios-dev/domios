import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { CredentialOwnerRecovery, credentialOwnerFailure } from "./CredentialOwnerRecovery";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, HTMLElement: window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}

assert.equal(credentialOwnerFailure("Sign-in did not complete: localized reason (os error 1307)"), true);
assert.equal(credentialOwnerFailure("ERROR_INVALID_OWNER"), true);
for (const reason of [undefined, "Access denied (os error 5)", "HTTP 1307", "os error 13070"]) {
  assert.equal(credentialOwnerFailure(reason), false);
}

async function main() {
  let finish!: () => void;
  let calls = 0;
  const onRecover = () => { calls++; return new Promise<void>(resolve => { finish = resolve; }); };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    const props = { tabId: "own", loginId: "attempt", error: "os error 1307", onRecover };
    await act(async () => root.render(<CredentialOwnerRecovery {...props} />));
    assert.equal(calls, 0);
    const button = host.querySelector<HTMLButtonElement>("button")!;
    assert.equal(button.textContent, "Move saved sign-in to backup");
    await act(async () => { button.click(); button.click(); });
    assert.equal(calls, 1);
    assert.equal(button.disabled, true);
    await act(async () => finish());
    assert.equal(host.querySelector("button"), null);
    assert.match(host.querySelector('[role="status"]')!.textContent!, /Choose a sign-in method/);
    await act(async () => root.render(<CredentialOwnerRecovery {...props} loginId="new-attempt" />));
    assert.notEqual(host.querySelector("button"), null);
    await act(async () => root.render(<CredentialOwnerRecovery {...props} loginId={undefined} />));
    assert.equal(host.textContent, "");
    const failure = async () => { calls++; throw new Error("The saved sign-in changed. Check again."); };
    await act(async () => root.render(<CredentialOwnerRecovery {...props} loginId="failure-attempt" onRecover={failure} />));
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    assert.equal(calls, 2);
    assert.match(host.textContent!, /The saved sign-in changed. Check again./);
    assert.equal(host.querySelector<HTMLButtonElement>("button")!.disabled, false);
  } finally {
    await act(async () => root.unmount()); host.remove(); await window.happyDOM.close();
  }
}
await main();
