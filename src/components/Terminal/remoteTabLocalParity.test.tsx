import { readFileSync } from "node:fs";
import { describe, expect, finish, it } from "../../test/standaloneTest";

// Tokens assembled at runtime so a literal grep for the retired legacy symbol over
// src stays zero — it must not appear verbatim even inside this guard.
const LEGACY_FIELD = ["remote", "availability"].join("_");
const LEGACY_ACTION = ["UPDATE", "REMOTE", "AVAILABILITY"].join("_");
const LEGACY_TERMINAL_GATE = `remote.${LEGACY_FIELD} !== "available"`;

describe("remote tab local component parity", () => {
  it("uses TerminalTab, TabSessionChat and TabTopBar without a parallel remote surface", () => {
    const source = readFileSync(new URL("./TabContent.tsx", import.meta.url), "utf8");
    expect(source).toContain("<TerminalTab");
    expect(source).toContain("<TabSessionChat");
    expect(source).toContain("<TabTopBar");
    expect(source).not.toContain("RemoteTabSurface");
    expect(source).not.toContain("RemoteTabUnavailable");
    expect(source).not.toContain(LEGACY_FIELD);
    const storeSource = readFileSync(new URL("../../store/multiplexer/index.tsx", import.meta.url), "utf8");
    expect(storeSource).not.toContain(LEGACY_TERMINAL_GATE);
    expect(storeSource).not.toContain(LEGACY_ACTION);
    const routeSource = readFileSync(new URL("../../lib/remoteTabRoute.ts", import.meta.url), "utf8");
    expect(routeSource).not.toContain(LEGACY_FIELD);
    // Legacy singular availability is fully retired: only per-channel remote_channels remains.
    const reducerSource = readFileSync(new URL("../../store/multiplexer/reducer.ts", import.meta.url), "utf8");
    expect(reducerSource).not.toContain(LEGACY_FIELD);
    expect(reducerSource).not.toContain(LEGACY_ACTION);
    const typesSource = readFileSync(new URL("../../lib/types.ts", import.meta.url), "utf8");
    expect(typesSource).not.toContain(LEGACY_FIELD);
  });

  it("gates 'Loading chat' on the chat channel only, never on PTY/output", () => {
    const chatSource = readFileSync(new URL("./TabSessionChat.tsx", import.meta.url), "utf8");
    // The chat loading gate reads the CHAT channel state.
    expect(chatSource).toContain("remote_channels?.chat");
    expect(chatSource).toContain("chatChannelLoading");
    // It must never gate chat loading on the terminal (PTY/output/control) channels.
    expect(chatSource).not.toContain("remoteTerminalUsable");
    expect(chatSource).not.toContain("remote_channels?.output");
    expect(chatSource).not.toContain("remote_channels?.control");
  });
});

finish();
