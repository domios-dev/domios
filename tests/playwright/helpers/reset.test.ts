import assert from "node:assert/strict";
import test from "node:test";
import type { ApiClient } from "./api-client";
import { apiReset, assertApiResetAllowed } from "./reset";
import type { DevInstance } from "./dev-instance";

function instance(name: string, dataDirName: string): DevInstance {
  return {
    slot: 0,
    name,
    apiPort: 7786,
    vitePort: 1420,
    cdpPort: 9223,
    identifier: `com.codeterm.dev.${name}`,
    dataDirName,
    apiBase: "http://127.0.0.1:7786",
    cdpBase: "http://127.0.0.1:9223",
    webUrl: "http://127.0.0.1:7786/",
  };
}

test("apiReset only accepts known disposable Playwright data directories", async () => {
  const api = {} as ApiClient;
  for (const safe of [
    instance("playwright", ".codeterm-dev-playwright"),
    instance("pwserial", ".codeterm-dev-pwserial"),
    instance("pw2", ".codeterm-dev-pw2"),
  ]) {
    assert.doesNotThrow(() => assertApiResetAllowed(safe));
  }

  for (const unsafe of [
    instance("", ".codeterm-dev"),
    instance("coreqa", ".codeterm-dev-coreqa"),
    instance("pwqa", ".codeterm-dev-pwqa"),
    instance("playwright", ".codeterm-dev-coreqa"),
  ]) {
    await assert.rejects(() => apiReset(api, unsafe), /apiReset refuses non-disposable data directory/);
  }
});
