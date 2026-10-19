import assert from "node:assert/strict";
import type { TransportClient } from "../transportClient";
import { configureOrchestrationTransport, listenOrchestration } from "./orchestrationRuntime";

type Handler = (name: string, payload: unknown) => void;
let opened = 0;
let closed = 0;
let emit: Handler = () => {};
const client = {
  json: async () => ({}),
  subscribe: (_path: string, handler: Handler) => {
    opened++;
    emit = handler;
    return () => { closed++; };
  },
} as unknown as TransportClient;

configureOrchestrationTransport(client);

void (async () => {
  const seen: string[] = [];
  const stopA = await listenOrchestration<string>("cron-run-finished", ({ payload }) => seen.push(`a:${payload}`));
  const stopB = await listenOrchestration<string>("cron-run-finished", ({ payload }) => seen.push(`b:${payload}`));
  const stopC = await listenOrchestration<string>("automation-graph-changed", ({ payload }) => seen.push(`c:${payload}`));
  assert.equal(opened, 1, "every listener shares one daemon stream");

  emit("cron-run-finished", "1");
  emit("automation-graph-changed", "2");
  emit("unrelated", "3");
  assert.deepEqual(seen, ["a:1", "b:1", "c:2"]);

  stopA();
  stopA();
  stopB();
  assert.equal(closed, 0, "the stream stays open while a listener remains");
  stopC();
  assert.equal(closed, 1, "the last listener closes the stream once");

  const stopD = await listenOrchestration("cron-run-finished", () => {});
  assert.equal(opened, 2, "a listener after the close reopens one stream");
  stopD();
})().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
