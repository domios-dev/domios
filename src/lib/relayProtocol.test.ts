import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, finish, it } from "../test/standaloneTest";
import { RELAY_PROTOCOL_VERSION } from "./types";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

describe("relay protocol version", () => {
  it("matches the version the server announces to peers", () => {
    const source = readFileSync(
      path.join(REPO_ROOT, "src-tauri", "crates", "codeterm-mesh", "src", "contract", "types.rs"),
      "utf8",
    );
    const announced = source.match(/pub const RELAY_PROTOCOL_VERSION: u16 = (\d+);/);
    expect(announced).not.toBeNull();
    expect(RELAY_PROTOCOL_VERSION).toBe(Number(announced?.[1]));
  });
});

finish();
