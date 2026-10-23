import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import connection from "../../evals/_lib/connection.cjs";

const dataDir = path.resolve("fixture/.domios-dev-isolated");

test("DEV consumers use the library and the selected instance instead of inherited authority", () => {
  let observed;
  const result = connection.devConnection({ dataDir, apiPort: 7791, instance: "Isolated" }, (binary, args, options) => {
    observed = { binary, args, options };
    return JSON.stringify({ dataDir, api: "http://127.0.0.1:7791", token: "selected-instance" });
  });
  assert.equal(result.token, "selected-instance");
  assert.equal(observed.options.env.CODETERM_INSTANCE, "Isolated");
  assert.equal(observed.options.env.CODETERM_API_PORT, "7791");
  assert.match(observed.args[1], /source "\$1"; dev_run/);
  assert.equal(path.basename(observed.args.at(-1)), "lib.sh");
});

test("DEV consumers reject credentials resolved from a different instance", () => {
  assert.throws(() => connection.devConnection({ dataDir }, () => JSON.stringify({
    dataDir: path.resolve("fixture/.domios-dev-other"), token: "wrong-instance",
  })), /different data directory/);
});

test("DEV credential errors propagate without an environment fallback", () => {
  assert.throws(() => connection.devConnection({}, () => { throw new Error("store unavailable"); }), /store unavailable/);
  assert.throws(() => connection.devConnection({}, () => JSON.stringify({ token: "" })), /Missing or invalid/);
});
