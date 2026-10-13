import { describe, expect, it } from "vitest";
import { pluginUiDocumentState, pluginUiHostTransition, pluginUiRuntimeEvent } from "./pluginUiHostModel";

describe("plugin UI host model", () => {
  it("does not treat a fetched document or bridge handshake as rendered content", () => {
    expect(pluginUiDocumentState(undefined, null, { status: "ready" })).toEqual({ status: "loading" });
    expect(pluginUiDocumentState("<div id='ct-root'></div>", null, { status: "loading" })).toEqual({ status: "loading" });
    expect(pluginUiRuntimeEvent({ __ctReady: true })).toBeNull();
  });
  it("exposes failed bundle loads and empty documents", () => {
    expect(pluginUiDocumentState(undefined, "bundle HTTP 404", { status: "loading" })).toEqual({ status: "error", reason: "bundle HTTP 404" });
    expect(pluginUiDocumentState(" ", null, { status: "loading" }).status).toBe("empty");
  });
  it("keeps a mount exception visible until an explicit retry", () => {
    const failed = pluginUiHostTransition({ status: "loading" }, { type: "error", reason: "Target container is not a DOM element." });
    expect(pluginUiHostTransition(failed, { type: "ready" })).toEqual(failed);
    expect(pluginUiHostTransition(failed, { type: "load" })).toEqual({ status: "loading" });
  });
  it("allows an asynchronous plugin to fill its initially empty mount", () => {
    expect(pluginUiHostTransition({ status: "empty", reason: "no content" }, { type: "ready" })).toEqual({ status: "ready" });
  });
  it("rejects malformed runtime messages", () => {
    for (const input of [null, "ready", { status: "error" }, { status: "error", reason: "" }]) expect(pluginUiRuntimeEvent(input)).toBeNull();
    expect(pluginUiRuntimeEvent({ status: "error", reason: "mount failure" })).toEqual({ type: "error", reason: "mount failure" });
  });
});
