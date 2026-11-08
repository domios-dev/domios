import { expect, it, vi } from "vitest";
import { createChatErrorNotice } from "./chatErrorNotice";

it("publishes once per failure episode, clears on recovery and allows a later failure", () => {
  const upsertToast = vi.fn(), dismissToast = vi.fn();
  const notice = createChatErrorNotice("own", { upsertToast, dismissToast });
  notice.show("opaque"); notice.show("opaque");
  expect(upsertToast).toHaveBeenCalledTimes(1);
  expect(upsertToast.mock.calls[0][0]).toMatchObject({ id: "chat-error:own", lifetime_ms: 10_000 });
  notice.recover(); expect(dismissToast).toHaveBeenCalledWith("chat-error:own");
  notice.show("opaque"); expect(upsertToast).toHaveBeenCalledTimes(2);
});

it("recovery clears only its chat and never publishes a new failure", () => {
  const upsertToast = vi.fn(), dismissToast = vi.fn();
  const notice = createChatErrorNotice("other", { upsertToast, dismissToast });
  notice.show("opaque"); notice.recover(); notice.recover();
  expect(dismissToast.mock.calls).toEqual([["chat-error:other"]]);
  expect(upsertToast).toHaveBeenCalledTimes(1);
});
