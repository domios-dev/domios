import assert from "node:assert/strict";
import { claudeProjectKey, claudeTrustRecord } from "../src/preTrust";
import type { Host, PreTrustOutcome } from "@codeterm/plugin-sdk";

assert.equal(claudeProjectKey("/private/tmp/trustprobe.d1AhFE", false), "/private/tmp/trustprobe.d1AhFE");
assert.equal(claudeProjectKey("c:\\Users\\alex\\src\\demo", true), "C:/Users/alex/src/demo");
assert.equal(claudeProjectKey("\\\\server\\share\\project", true), "//server/share/project");

for (const platform of ["macos", "linux", "windows"]) {
  const account = platform === "windows" ? "C:/account" : "/account";
  const home = platform === "windows" ? "C:/home" : "/home";
  for (const config of [null, "", account + "/"]) {
    for (const status of ["already_trusted", "granted", "skipped", "failed"] as const) {
      const expected: PreTrustOutcome = status === "skipped" || status === "failed"
        ? { status, reason: "opaque" } : { status };
      const calls: unknown[][] = [];
      const stub: Pick<Host, "envGet" | "homeDir" | "platform" | "canonicalPath" | "jsonBooleanRecord"> = {
        envGet: () => config,
        homeDir: () => home,
        platform: () => platform,
        canonicalPath: (path) => {
          assert.equal(path, "/tmp/alias");
          return platform === "windows" ? "c:\\Users\\alex\\src\\demo" : "/private/tmp/real";
        },
        jsonBooleanRecord: (path, keys, grant) => {
          calls.push([path, JSON.parse(keys), grant]);
          return JSON.stringify(expected);
        },
      };
      assert.deepEqual(claudeTrustRecord(stub, "/tmp/alias", true), expected);
      assert.deepEqual(claudeTrustRecord(stub, "/tmp/alias", false), expected);
      const key = platform === "windows" ? "C:/Users/alex/src/demo" : "/private/tmp/real";
      assert.deepEqual(calls, [true, false].map((grant) => [
        (config ? account : home) + "/.claude.json",
        ["projects", key, "hasTrustDialogAccepted"], grant,
      ]));
    }
  }
}

const unresolved = {
  envGet: () => null, homeDir: () => "/home", platform: () => "linux",
  canonicalPath: () => null,
  jsonBooleanRecord: () => { throw new Error("unresolved cwd must never reach the writer"); },
};
assert.equal(claudeTrustRecord(unresolved, "/missing", true).status, "skipped");
assert.equal(claudeTrustRecord({ ...unresolved, canonicalPath: () => "/real" }, "/alias", true).status, "failed");
console.log("pre-trust: passed");

assert.equal(claudeTrustRecord({ ...unresolved, envGet: () => "relative-account-root", canonicalPath: () => "/real" }, "/alias", true).status, "skipped");
