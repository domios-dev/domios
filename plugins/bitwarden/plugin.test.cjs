// Plugin-side tests for the Bitwarden secret-backend plugin.
// Run: npx tsx plugins/bitwarden/plugin.test.cjs
//
// Bitwarden behaviour lives with the plugin, not in CodeTerm core — core only
// tests the generic JsSecretBackend seam. These exercise the pure helpers
// (no host.exec): base64, bw error-message mapping, list-envelope unwrap, and
// login-cipher shape.

// The plugin references `host` only inside methods, never at module load, so a
// bare require is safe. Stub it anyway so an accidental top-level call is loud.
globalThis.host = new Proxy(
  {},
  { get: () => () => { throw new Error("host called at load time"); } },
);

const plugin = require("./plugin.js").default;

const tests = [];
function test(name, fn) { tests.push([name, fn]); }
function assert(cond, msg) { if (!cond) throw new Error(msg); }

test("base64 — standard encoding", () => {
  assert(plugin.__test_base64("hello") === "aGVsbG8=", "base64 of 'hello'");
});

test("bw error-message mapping", () => {
  assert(plugin.__test_mapError("Vault is locked.").kind === "locked", "locked");
  assert(plugin.__test_mapError("Item Not found.").kind === "not_found", "not_found");
  assert(plugin.__test_mapError("You are not logged in.").kind === "logged_out", "logged_out");
  assert(
    plugin.__test_mapError("An item with that name already exists.").kind === "already_exists",
    "already_exists",
  );
});

test("list-envelope unwrap", () => {
  const out = plugin.__test_unwrap({ object: "list", data: [1, 2] });
  assert(Array.isArray(out) && out.length === 2 && out[0] === 1 && out[1] === 2, "unwrap data array");
});

test("login-cipher shape (type=1, password in login)", () => {
  const cipher = plugin.__test_buildCipher({ name: "n", value: "p", notes: null }, null, null);
  assert(cipher.type === 1, "type 1 login");
  assert(cipher.name === "n", "name");
  assert(cipher.login.password === "p", "password in login");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(err);
  }
}
console.log(`bitwarden plugin: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
