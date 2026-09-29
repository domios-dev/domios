import { describe, expect, finish, it } from "../test/standaloneTest";
import { RemotePtyIdCache, writeRemotePty } from "./remotePtyIdCache";

describe("remote PTY id cache", () => {
  it("resolves once for repeated writes and refreshes after invalidation", async () => {
    const cache = new RemotePtyIdCache();
    let lookups = 0;
    const lookup = async () => `pty-${++lookups}`;

    expect(await cache.resolve("mesh:peer/tab", lookup)).toBe("pty-1");
    expect(await cache.resolve("mesh:peer/tab", lookup)).toBe("pty-1");
    expect(lookups).toBe(1);

    cache.invalidate("mesh:peer/tab");
    expect(await cache.resolve("mesh:peer/tab", lookup)).toBe("pty-2");
    expect(lookups).toBe(2);
  });

  it("uses one relay call per write after the initial PTY resolve", async () => {
    const cache = new RemotePtyIdCache();
    const calls: string[] = [];
    const resolve = async () => {
      calls.push("inspect");
      return "pty-a";
    };
    const write = async (ptyId: string) => {
      calls.push(`write:${ptyId}`);
      return { ok: true as const, data: undefined };
    };

    await writeRemotePty(cache, "mesh:peer/tab", resolve, write);
    await writeRemotePty(cache, "mesh:peer/tab", resolve, write);

    expect(calls).toEqual(["inspect", "write:pty-a", "write:pty-a"]);
  });

  it("invalidates a cached PTY only when its write reports it missing", async () => {
    const cache = new RemotePtyIdCache();
    let lookups = 0;
    const resolve = async () => `pty-${++lookups}`;
    const failure = async (status: number) =>
      writeRemotePty(cache, "mesh:peer/tab", resolve, async () => ({
        ok: false,
        status,
        error: `HTTP ${status}`,
      }));

    await expect(failure(502)).rejects.toThrow("HTTP 502");
    expect(await cache.resolve("mesh:peer/tab", resolve)).toBe("pty-1");
    await expect(failure(404)).rejects.toThrow("HTTP 404");
    expect(await cache.resolve("mesh:peer/tab", resolve)).toBe("pty-2");
    expect(lookups).toBe(2);
  });

  it("shares an in-flight lookup", async () => {
    const cache = new RemotePtyIdCache();
    let releaseLookup!: (ptyId: string) => void;
    let lookups = 0;
    const firstLookup = new Promise<string>((resolve) => {
      releaseLookup = resolve;
    });
    const lookup = () => {
      lookups += 1;
      return firstLookup;
    };

    const first = cache.resolve("mesh:peer/tab", lookup);
    const second = cache.resolve("mesh:peer/tab", lookup);
    releaseLookup("pty-current");
    expect(await first).toBe("pty-current");
    expect(await second).toBe("pty-current");
    expect(lookups).toBe(1);
  });

  it("does not cache a rejected lookup", async () => {
    const cache = new RemotePtyIdCache();
    let lookups = 0;
    const failed = cache.resolve("mesh:peer/tab", async () => {
      lookups += 1;
      throw new Error("inspect failed");
    });
    expect((await Promise.allSettled([failed]))[0]?.status).toBe("rejected");
    expect(await cache.resolve("mesh:peer/tab", async () => {
      lookups += 1;
      return "pty-recovered";
    })).toBe("pty-recovered");
    expect(lookups).toBe(2);
  });
});

finish();
