import assert from "node:assert/strict";
import { finish, it } from "../test/standaloneTest";
import { adoptStoredActions } from "./notificationActions";

it("stored prompt options retain their value and prompt identity for the answer route", () => {
  const identity = {
    kind: "directory_trust",
    options: [
      { label: "Trust and continue", value: "1" },
      { label: "Quit", value: "2" },
    ],
  };
  assert.deepEqual(
    adoptStoredActions([
      { type: "respond_prompt_option", label: "Trust and continue", value: "1", identity },
      { type: "respond_prompt_option", label: "Quit", value: "2", identity },
    ]),
    [
      { type: "respond_prompt_option", label: "Trust and continue", value: "1", identity },
      { type: "respond_prompt_option", label: "Quit", value: "2", identity },
    ],
  );
});

it("malformed prompt response actions are dropped", () => {
  assert.equal(
    adoptStoredActions([
      { type: "respond_prompt_option", label: "Trust", value: "1", identity: { kind: "directory_trust", options: [] } },
    ])?.length,
    0,
  );
});

finish();
