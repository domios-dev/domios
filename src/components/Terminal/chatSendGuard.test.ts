import { DUPLICATE_SEND_WINDOW_MS, isDuplicateSend } from "./chatSendGuard";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const stamp = { text: "hello attachment.png", at: 10_000 };

assert(
  isDuplicateSend(stamp, "hello attachment.png", 10_000 + DUPLICATE_SEND_WINDOW_MS - 1),
  "identical payload inside the window is a duplicate",
);
assert(
  !isDuplicateSend(stamp, "hello attachment.png", 10_000 + DUPLICATE_SEND_WINDOW_MS),
  "the window closing allows an intentional resend",
);
assert(!isDuplicateSend(stamp, "hello other.png", 10_100), "different payload is never a duplicate");
console.log("chat send guard checks passed");
