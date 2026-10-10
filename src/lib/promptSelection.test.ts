import assert from "node:assert/strict";
import { finish, it } from "../test/standaloneTest";
import { encodePromptSelection } from "./promptSelection";

it("encodes every selected option into the existing small-menu mask", () => {
  const options = ["1", "2", "3"].map((value) => ({ label: value, value }));
  assert.equal(encodePromptSelection(options, new Set(["1", "3"])), "10005");
  assert.equal(encodePromptSelection(options, new Set()), "10000");
});

it("large selections retain their values without bit wrapping", () => {
  const options = Array.from({ length: 40 }, (_, index) => ({ label: String(index), value: String(index + 1) }));
  assert.deepEqual(JSON.parse(encodePromptSelection(options, new Set(["1", "33", "40"]))), ["1", "33", "40"]);
});

it("opaque option values remain opaque in an ordered selection array", () => {
  const options = ["opaque-a", "opaque-b"].map((value) => ({ label: value, value }));
  assert.deepEqual(JSON.parse(encodePromptSelection(options, new Set(["opaque-b"]))), ["opaque-b"]);
});

finish();
