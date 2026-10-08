import assert from "node:assert/strict";
import { after, test } from "node:test";
import { Window } from "happy-dom";
import { act } from "react";
import type { ModelEndpointsApi } from "./modelEndpointsApi";
import type { AddRequest, DiscoverRequest, ModelEndpoint, UpdateRequest } from "./modelEndpointsModel";

// react-dom decides input-event support when it loads, so the DOM must exist before it is imported.
const window = new Window({ url: "http://localhost" });
for (const key of ["document", "navigator", "HTMLElement", "HTMLInputElement", "HTMLSelectElement", "HTMLButtonElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: (window as never)[key], configurable: true });
}
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
after(() => window.happyDOM.close());

const KEY = ["opaque", "typed", "key"].join("-");

function fakeApi(initial: ModelEndpoint[] = []) {
  let rows = [...initial];
  const calls: { op: string; request?: unknown }[] = [];
  const api: ModelEndpointsApi = {
    list: async () => rows,
    add: async (request: AddRequest) => {
      calls.push({ op: "add", request });
      rows = [...rows, { ...request.provider, models: request.provider.models.length ? request.provider.models : ["alpha", "beta"], secretName: `domios/model-provider/${request.provider.id}` }];
    },
    update: async (request: UpdateRequest) => {
      calls.push({ op: "update", request });
      rows = rows.map((row) => (row.id === request.provider.id ? { ...row, ...request.provider } : row));
    },
    remove: async (id) => { calls.push({ op: "remove", request: id }); rows = rows.filter((row) => row.id !== id); },
    test: async (id) => { calls.push({ op: "test", request: id }); return ["alpha", "beta"]; },
    discover: async (request: DiscoverRequest) => { calls.push({ op: "discover", request }); return ["alpha", "beta"]; },
    savedSecrets: async () => ["source-secret", "domios/model-provider/old"],
  };
  return { api, calls };
}

async function mount(api: ModelEndpointsApi) {
  const { createRoot } = await import("react-dom/client");
  const { ModelEndpointsCard } = await import("./ModelEndpointsCard");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<ModelEndpointsCard providerId="fixture-harness" harnessName="Fixture" api={api} />));
  const byText = (text: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.trim() === text) ?? null;
  const byLabel = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  const click = async (element: HTMLElement | null) => {
    assert.ok(element, "control exists");
    await act(async () => element.click());
  };
  const type = async (input: HTMLInputElement | HTMLSelectElement | null, value: string) => {
    assert.ok(input, "field exists");
    await act(async () => {
      Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), "value")!.set!.call(input, value);
      input.dispatchEvent(new window.Event(input.tagName === "SELECT" ? "change" : "input", { bubbles: true }) as unknown as Event);
    });
  };
  const field = <T extends HTMLElement = HTMLInputElement>(labelText: string) => {
    const label = [...host.querySelectorAll("label")].find((node) => node.textContent?.trim() === labelText);
    const id = label?.getAttribute("for");
    return id ? ([...host.querySelectorAll<T>("[id]")].find((node) => node.id === id) ?? null) : null;
  };
  const submit = async () => {
    await act(async () => host.querySelector("form")!.dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }) as unknown as Event));
  };
  const close = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  return { host, byText, byLabel, click, type, field, submit, close };
}

test("adding an endpoint saves the typed key through the request and never renders it", async () => {
  const { api, calls } = fakeApi();
  const ui = await mount(api);
  try {
    await ui.click(ui.byText("Add endpoint"));
    await ui.type(ui.field("Name"), "lm-studio");
    await ui.type(ui.field("Base URL"), "http://localhost:1234/v1");
    const key = ui.field("API key");
    assert.equal(key?.getAttribute("type"), "password");
    await ui.type(key, KEY);
    await ui.click(ui.byText("Discover models"));
    const boxes = [...ui.host.querySelectorAll<HTMLInputElement>('ul[aria-label="Discovered models"] input[type="checkbox"]')];
    assert.equal(boxes.length, 2);
    assert.equal(ui.host.innerHTML.includes(KEY), false);
    await ui.click(boxes[1]);
    await ui.submit();
    const add = calls.find((call) => call.op === "add")?.request as AddRequest;
    assert.equal(add.action, "add");
    assert.equal(add.action === "add" && add.value, KEY);
    assert.deepEqual(add.provider.models, ["beta"]);
    assert.equal(ui.host.querySelector("form"), null);
    assert.ok(ui.host.querySelector('[data-endpoint-id="lm-studio"]'));
    assert.equal(ui.host.innerHTML.includes(KEY), false);
    assert.deepEqual(calls.map((call) => call.op), ["discover", "add"]);
  } finally {
    await ui.close();
  }
});

test("a saved secret is picked by name and managed copies are not offered", async () => {
  const { api, calls } = fakeApi();
  const ui = await mount(api);
  try {
    await ui.click(ui.byText("Add endpoint"));
    await ui.type(ui.field("Name"), "router");
    await ui.type(ui.field("Base URL"), "https://example.test/v1");
    await ui.click(ui.byText("Saved secret"));
    const select = ui.field<HTMLSelectElement>("API key");
    assert.ok(select);
    const options = [...select.querySelectorAll("option")].map((option) => option.value).filter(Boolean);
    assert.deepEqual(options, ["source-secret"]);
    await ui.type(select, "source-secret");
    await ui.submit();
    const add = calls.find((call) => call.op === "add")?.request as AddRequest;
    assert.equal(add.action, "add_from_secret");
    assert.equal(add.action === "add_from_secret" && add.source_secret, "source-secret");
  } finally {
    await ui.close();
  }
});

test("existing endpoints test, edit and remove from their own row", async () => {
  const row: ModelEndpoint = { id: "fixture", kind: "openai-compatible", baseUrl: "http://localhost:1234/v1", models: ["alpha"], secretName: "domios/model-provider/fixture" };
  const { api, calls } = fakeApi([row]);
  const ui = await mount(api);
  try {
    assert.ok(ui.host.textContent?.includes("domios/model-provider/fixture"));
    await ui.click(ui.byLabel("Test connection to fixture"));
    assert.ok(ui.host.querySelector('[role="status"]'));
    await ui.click(ui.byLabel("Edit fixture"));
    await ui.type(ui.field("Base URL"), "http://localhost:4321/v1");
    await ui.submit();
    const update = calls.find((call) => call.op === "update")?.request as UpdateRequest;
    assert.equal(update.provider.baseUrl, "http://localhost:4321/v1");
    assert.equal("value" in update, false);
    await ui.click(ui.byLabel("Remove fixture"));
    await ui.click(ui.byLabel("Confirm remove fixture"));
    assert.equal(ui.host.querySelector('[data-endpoint-id="fixture"]'), null);
    assert.deepEqual(calls.map((call) => call.op), ["test", "update", "remove"]);
  } finally {
    await ui.close();
  }
});

test("a rejected key shows one line of guidance on the form", async () => {
  const { api } = fakeApi();
  api.discover = async () => { throw "unauthorized"; };
  const ui = await mount(api);
  try {
    await ui.click(ui.byText("Add endpoint"));
    await ui.type(ui.field("Base URL"), "http://localhost:1234/v1");
    await ui.type(ui.field("API key"), KEY);
    await ui.click(ui.byText("Discover models"));
    assert.equal(ui.host.querySelectorAll('[role="alert"]').length, 1);
    assert.equal(ui.host.innerHTML.includes(KEY), false);
  } finally {
    await ui.close();
  }
});
