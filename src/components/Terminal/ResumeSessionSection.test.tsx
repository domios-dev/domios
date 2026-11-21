import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { ResumeSessionSection } from "./ResumeSessionSection";
import type { ResumableSessionsTransport } from "../../lib/resumableSessions";

test("ResumeSessionSection stays hidden when no resumable sessions exist for cwd", async () => {
  const transport: ResumableSessionsTransport = {
    json: async () => ({ hits: [] }) as never,
  };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => {
      root.render(
        <ResumeSessionSection
          workspaceId="ws-1"
          cwd="/work/project"
          disabled={false}
          providerLabel={(id) => id}
          onResumed={() => {}}
          onError={() => {}}
          transport={transport}
        />,
      );
    });
    assert.equal(host.querySelector('[data-testid="picker-resume-sessions"]'), null);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("ResumeSessionSection lists unified Claude/Codex and ACP sessions and resumes on click", async () => {
  const nowSec = Math.floor(Date.now() / 1000) - 300;
  const calls: Array<[string, string, unknown]> = [];
  const transport: ResumableSessionsTransport = {
    json: async (method, path, body) => {
      calls.push([method, path, body]);
      if (method === "GET") {
        return {
          hits: [
            { meta: { session_id: "claude-1", provider: "claude", title: "Audit routes", project_path: "/work/project", modified_ts: nowSec } },
            { meta: { session_id: "acp-1", provider: "acp", title: "ACP Live UX", project_path: "/work/project", modified_ts: nowSec - 600 } },
          ],
        } as never;
      }
      return { tab_id: "tab-resumed", workspace_id: "ws-1" } as never;
    },
  };
  const resumed: Array<{ workspaceId: string; tabId?: string }> = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => {
      root.render(
        <ResumeSessionSection
          workspaceId="ws-1"
          cwd="/work/project"
          disabled={false}
          providerLabel={(id) => (id === "acp" ? "ACP" : "Claude")}
          onResumed={(result) => { resumed.push(result); }}
          onError={() => {}}
          transport={transport}
        />,
      );
    });
    const items = host.querySelectorAll<HTMLButtonElement>('[data-testid="picker-resume-session-item"]');
    assert.equal(items.length, 2);
    assert.ok(items[0].textContent?.includes("Audit routes"));
    assert.ok(items[1].textContent?.includes("ACP Live UX"));
    assert.ok(items[1].textContent?.includes("ACP"));
    await act(async () => items[1].click());
    assert.deepEqual(calls[1], ["POST", "/api/sessions/acp-1/resume", { workspace_id: "ws-1" }]);
    assert.deepEqual(resumed, [{ workspaceId: "ws-1", tabId: "tab-resumed" }]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
