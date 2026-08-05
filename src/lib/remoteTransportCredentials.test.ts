import { describe, expect, finish, it, vi } from "../test/standaloneTest";
import {
  remoteOutputSubscriptionSnapshot,
  subscribeTabOutput,
  waitForRemoteOutputGenerationSettlement,
} from "./outputStream";
import { configureRemoteTabTransport } from "./remoteTabTransport";

const PRELOAD = { enabled: true, host: "127.0.0.1", port: 7685, token: null, cors: false };
const LOADED = { ...PRELOAD, token: "loaded-token" };

const authOf = (call: unknown[]): string => {
  const init = call[1] as { headers?: Record<string, string> } | undefined;
  return init?.headers?.Authorization ?? "";
};

describe("remote transport credentials", () => {
  it("channel darkened without a token revives when the real token arrives", async () => {
    vi.stubGlobal("window", new EventTarget());
    const fetchMock = vi.fn()
      .mockReturnValueOnce(Promise.resolve(new Response(null, { status: 401 })))
      .mockReturnValue(new Promise(() => {}));
    vi.stubGlobal("fetch", fetchMock);
    configureRemoteTabTransport(PRELOAD);

    const id = "mesh:peer/cred1";
    const unsubscribe = subscribeTabOutput(id, { onOutput() {}, onTruncated() {}, onEnded() {} });
    const darkened = remoteOutputSubscriptionSnapshot(id).generation;
    await waitForRemoteOutputGenerationSettlement(id, darkened);
    expect(remoteOutputSubscriptionSnapshot(id)).toEqual({ generation: darkened, hasController: false });

    configureRemoteTabTransport(LOADED);
    expect(remoteOutputSubscriptionSnapshot(id)).toEqual({ generation: darkened + 1, hasController: true });
    expect(authOf(fetchMock.mock.calls[0])).toBe("");
    expect(authOf(fetchMock.mock.calls[1])).toBe(`Bearer ${LOADED.token}`);

    unsubscribe();
    vi.unstubAllGlobals();
  });

  it("a config re-emit carrying the same credentials does not churn the connection", async () => {
    vi.stubGlobal("window", new EventTarget());
    vi.stubGlobal("fetch", vi.fn().mockReturnValue(new Promise(() => {})));
    configureRemoteTabTransport(LOADED);

    const id = "mesh:peer/cred2";
    const unsubscribe = subscribeTabOutput(id, { onOutput() {}, onTruncated() {}, onEnded() {} });
    const live = remoteOutputSubscriptionSnapshot(id).generation;
    configureRemoteTabTransport({ ...LOADED });
    expect(remoteOutputSubscriptionSnapshot(id)).toEqual({ generation: live, hasController: true });

    unsubscribe();
    vi.unstubAllGlobals();
  });
});

finish();
