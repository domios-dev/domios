import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

test("shared error presentation guard and its rejection fixtures pass", () => {
  execFileSync(process.execPath, ["--test", "scripts/check-error-presentation.test.mjs"], {
    cwd: fileURLToPath(new URL("../../", import.meta.url)), stdio: "inherit",
  });
});
