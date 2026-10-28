import assert from "node:assert/strict";
import { test } from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Window } from "happy-dom";
import { BrandState } from "./BrandState";
import { ServerConnectionLayerView } from "./ServerConnectionLayer";
import { brandConnectionModel, brandStateModel, RECONNECT_GRACE_MS } from "./brandStateModel";

const window = new Window();

function parsed(markup: string) {
  const host = window.document.createElement("div");
  host.innerHTML = markup;
  return host;
}

test("loading is announced and includes the threshold motion and an animated matrix canvas", () => {
  const host = parsed(renderToStaticMarkup(<BrandState model={brandStateModel({ loading: true, label: "Restoring workspace" })} />));
  const status = host.querySelector('[role="status"]');
  assert.ok(status);
  assert.equal(status.getAttribute("aria-live"), "polite");
  assert.equal(status.getAttribute("aria-busy"), "true");
  assert.equal(status.getAttribute("aria-label"), "Restoring workspace");
  assert.equal(host.querySelector(".brand-state-matrix")?.getAttribute("aria-hidden"), "true");
  assert.equal(host.querySelector(".brand-state-matrix")?.getAttribute("data-wave-enabled"), "true");
  assert.equal(host.querySelector(".brand-mark path")?.getAttribute("d"), "M8 8H32C46 8 56 18 56 32S46 56 32 56H24L40 40V24H24V40L8 56Z");
});

test("error is a static alert and preserves its recovery action", () => {
  const host = parsed(renderToStaticMarkup(
    <BrandState model={brandStateModel({ error: { title: "Could not start", sentence: "Retry the session." } })}>
      <button type="button">Retry</button>
    </BrandState>,
  ));
  const alert = host.querySelector('[role="alert"]');
  assert.ok(alert);
  assert.equal(alert.getAttribute("aria-live"), "assertive");
  const heading = alert.querySelector("h2");
  assert.equal(heading?.textContent, "Could not start");
  assert.equal(alert.getAttribute("aria-labelledby"), heading?.getAttribute("id"));
  assert.equal(alert.querySelector("p")?.textContent, "Retry the session.");
  assert.equal(host.querySelector(".brand-state-matrix")?.getAttribute("data-wave-enabled"), "false");
  assert.equal(host.querySelector("button")?.textContent, "Retry");
});

test("reduced motion keeps the mark and faint matrix static without waves", () => {
  const original = globalThis.matchMedia;
  globalThis.matchMedia = (() => ({ matches: true, addEventListener() {}, removeEventListener() {} })) as unknown as typeof matchMedia;
  try {
    const host = parsed(renderToStaticMarkup(<BrandState model={brandStateModel({ loading: true, label: "Loading" })} />));
    assert.equal(host.querySelector(".brand-state-matrix")?.getAttribute("data-wave-enabled"), "false");
    assert.equal(host.querySelector(".brand-state-matrix")?.getAttribute("aria-hidden"), "true");
    assert.equal(host.querySelector("[data-brand-state='open']"), null);
  } finally {
    globalThis.matchMedia = original;
  }
});

test("first-connect status blocks content without trapping focus", () => {
  const host = parsed(renderToStaticMarkup(
    <ServerConnectionLayerView model={brandConnectionModel({ kind: "disconnected", disconnectedAt: 0 }, RECONNECT_GRACE_MS, false)} contentClassName="content">
      <button type="button">Stale action</button>
    </ServerConnectionLayerView>,
  ));
  const content = host.querySelector(".content");
  const status = host.querySelector('[role="status"]');
  assert.ok(content?.hasAttribute("inert"));
  assert.ok(status);
  assert.equal(status.getAttribute("aria-live"), "polite");
  assert.equal(status.textContent?.replace(/\s+/g, " ").trim(), "Reconnecting…");
  assert.equal(host.querySelector(".brand-state-matrix"), null);
  assert.equal(host.querySelector("[aria-modal='true']"), null);
  assert.equal(status.querySelectorAll("button, a, input, select, textarea, [tabindex]").length, 0);
  const tab = new window.KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
  status.dispatchEvent(tab);
  assert.equal(tab.defaultPrevented, false);
});

test("a drop after load leaves the app usable and says updates are paused", () => {
  const host = parsed(renderToStaticMarkup(
    <ServerConnectionLayerView model={brandConnectionModel({ kind: "disconnected", disconnectedAt: 0 }, RECONNECT_GRACE_MS, true)} contentClassName="content">
      <button type="button">Loaded action</button>
    </ServerConnectionLayerView>,
  ));
  const content = host.querySelector(".content");
  const status = host.querySelector('[data-testid="connection-paused"]');
  assert.equal(content?.hasAttribute("inert"), false);
  assert.equal(host.querySelector(".brand-connection-scrim"), null);
  assert.ok(status);
  assert.equal(status.getAttribute("role"), "status");
  assert.equal(status.textContent?.replace(/\s+/g, " ").trim(), "Reconnecting…Updates paused");
  assert.equal(status.querySelectorAll("button, a, input, select, textarea, [tabindex]").length, 0);
});

window.close();
