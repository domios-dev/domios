import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { remoteTabKindLabel } from "./remoteTabKindLabel";

const tab = (id: string, fields: Partial<Tab> = {}): Tab => ({ id, pty_id: `pty-${id}`, tab_type: "Terminal", title: id, cwd: null, ...fields });

describe("remote tab role labels", () => {
  it("labels rows by role, then provider, then shell — never by backing kind", () => {
    expect(remoteTabKindLabel(tab("o", { role: "Manager", role_profile: "orchestrator", agent_type: "claude" }))).toBe("Orchestrator");
    expect(remoteTabKindLabel(tab("p", { role: "Manager", role_profile: "planner", agent_type: "codex" }))).toBe("Planner");
    expect(remoteTabKindLabel(tab("w", { role: "Worker", role_profile: null, agent_type: "codex" }))).toBe("Worker");
    expect(remoteTabKindLabel(tab("a", { agent_type: "codex" }))).toBe("Agent: Codex");
    expect(remoteTabKindLabel(tab("c", { pty_id: null, backing: { kind: "chat_backend", plugin_id: "local-model", session_id: "s" } }))).toBe("Agent: local-model");
    expect(remoteTabKindLabel(tab("s", { backing: { kind: "pty" } }))).toBe("Shell");
  });
});

finish();
