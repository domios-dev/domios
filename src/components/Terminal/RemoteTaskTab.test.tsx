import {act} from "react";
import {createRoot} from "react-dom/client";
import {expect, it, vi} from "vitest";
import {RemoteTaskTab} from "./RemoteTaskTab";
import type {RemoteTaskBinding, RemoteTaskCapability} from "../../../packages/plugin-sdk/src/remoteTasks";
import type {RemoteTaskCall} from "./useRemoteTask";

(globalThis as {IS_REACT_ACT_ENVIRONMENT?: boolean}).IS_REACT_ACT_ENVIRONMENT = true;
const capability: RemoteTaskCapability = {backend: "fixture", mode: "cloud", displayName: "Cloud", environments: [], availability: {status: "available"}, followUp: false, cancel: false, applyDiff: false};
const binding: RemoteTaskBinding = {
  revision: 1, providerId: "fixture", executionMode: "cloud", backend: "fixture", environmentId: "environment", accountRef: "account", branch: "master", phase: "bound",
  snapshot: {taskId: "task", attempt: null, status: "ready", result: "Fixture README title", diff: null, error: null, cursor: null},
};

it("refreshes a bound task once and shows the backend result without another submit", async () => {
  const host = document.createElement("div"); const root = createRoot(host);
  const call = vi.fn<RemoteTaskCall>(async method => method === "read" ? {...binding, snapshot: {...binding.snapshot!, status: "pending", result: ""}} : binding);
  try {
    await act(async () => root.render(<RemoteTaskTab tabId="remote-tab" pluginId="fixture" sessionId="tab" capability={capability} call={call} />));
    const refresh = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Refresh remote task");
    expect(refresh).toBeDefined();
    await act(async () => refresh?.click());
    expect(call.mock.calls.map(args => args[0])).toEqual(["read", "refresh"]);
    expect(host.querySelector("pre")?.textContent).toBe(binding.snapshot?.result);
    expect(host.querySelector("textarea")).toBeNull();
    expect(host.querySelector('button[type="submit"]')).toBeNull();
  } finally {await act(async () => root.unmount());}
});

it("shows unavailable provider state and refuses a submit without a live adapter", async () => {
  const host = document.createElement("div"); const root = createRoot(host);
  const call = vi.fn<RemoteTaskCall>(async () => null);
  const unavailable = {...capability, availability: {status: "unavailable" as const, reason: "Fixture account unavailable"}};
  try {
    await act(async () => root.render(<RemoteTaskTab tabId="remote-tab" pluginId="fixture" sessionId="tab" capability={unavailable} call={call} />));
    expect(host.querySelector<HTMLButtonElement>('button[type="submit"]')?.disabled).toBe(true);
    expect(host.querySelector<HTMLTextAreaElement>("textarea")?.disabled).toBe(true);
    expect(call.mock.calls.map(args => args[0])).toEqual(["read"]);
  } finally {await act(async () => root.unmount());}
});

it("keeps the new-task form usable and quiet when the background read fails", async () => {
  const host = document.createElement("div"); const root = createRoot(host);
  const call = vi.fn<RemoteTaskCall>(async method => {
    if (method === "read") throw new Error("temporary read failure");
    return binding;
  });
  try {
    await act(async () => root.render(<RemoteTaskTab tabId="remote-tab" pluginId="fixture" sessionId="tab" capability={capability} call={call} />));
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector('section[role="status"][aria-label]')).toBeNull();
    expect(host.querySelectorAll('[role="status"]')).toHaveLength(2);
    expect(host.querySelector<HTMLTextAreaElement>("textarea")?.disabled).toBe(false);
    expect(host.querySelector<HTMLButtonElement>('button[type="submit"]')?.disabled).toBe(true);
    const refresh = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Refresh remote task");
    expect(refresh?.disabled).toBe(false);
    await act(async () => refresh?.click());
    expect(host.querySelector<HTMLTextAreaElement>("textarea")).toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
  } finally {await act(async () => root.unmount());}
});

it("announces a failed explicit remote-task refresh once", async () => {
  const host = document.createElement("div"); const root = createRoot(host);
  const call = vi.fn<RemoteTaskCall>(async method => {
    if (method === "refresh") throw new Error("refresh denied");
    return binding;
  });
  try {
    await act(async () => root.render(<RemoteTaskTab tabId="remote-tab" pluginId="fixture" sessionId="tab" capability={capability} call={call} />));
    const refresh = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Refresh remote task");
    await act(async () => {
      refresh?.click();
      await vi.waitFor(() => expect(call.mock.calls.map(args => args[0])).toContain("refresh"));
    });
    expect(host.querySelectorAll('section[role="status"][aria-label]')).toHaveLength(1);
  } finally {await act(async () => root.unmount());}
});
