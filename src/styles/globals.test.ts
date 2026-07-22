import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const css = readFileSync(fileURLToPath(new URL("./globals.css", import.meta.url)), "utf8");

if (!css.includes('@source "../../packages/chat-ui/src";')) {
  throw new Error("Tailwind must scan shared Chat UI components");
}

console.log("shared Chat UI Tailwind source is configured");
