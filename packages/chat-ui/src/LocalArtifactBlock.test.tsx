import assert from "node:assert/strict";
import test from "node:test";
import { useLayoutEffect } from "react";
import { RichMarkdown, LocalArtifactScope, configureLocalArtifactLoader, mount } from "./dsBlocks.testdata";
import { createLocalArtifactMemory } from "./localArtifactMemory";

const yaml = "name: com.example/research-desk\nversion: 2.1.0\nrequires:\n  harnesses: [claude, codex]\n";
const never = () => new Promise<never>(() => {});

test("a remounted document preview shows its loaded file at once while it revalidates", async () => {
  const view = await mount();
  const reads: string[] = [];
  const committed: string[] = [];
  // Records the preview as first committed, before any effect could swap a placeholder for the file.
  function FirstLayout() {
    useLayoutEffect(() => { committed.push(view.host.textContent ?? ""); }, []);
    return null;
  }
  const preview = (tabId: string) => <LocalArtifactScope tabId={tabId}><RichMarkdown text={'<ds-document src="/teams/remount.yaml" />'} /><FirstLayout /></LocalArtifactScope>;
  try {
    configureLocalArtifactLoader(async (path) => { reads.push(path); return { data: yaml, media_type: "text/plain" }; });
    await view.render(preview("owner"));
    assert.ok(view.host.textContent?.includes("research-desk"));
    await view.render(null);

    configureLocalArtifactLoader((path) => { reads.push(path); return never(); });
    await view.render(preview("owner"));
    const remount = committed.at(-1) ?? "";
    assert.ok(remount.includes("research-desk") && !remount.includes("Loading…"), "the remount commits the loaded file, not a placeholder");
    assert.ok(view.host.textContent?.includes("research-desk"));
    assert.equal(reads.length, 2, "the remount still revalidates the file");
    await view.render(null);

    await view.render(preview("other"));
    assert.ok(committed.at(-1)?.includes("Loading…") && view.host.textContent?.includes("Loading…"), "another tab's scope never sees the remembered file");
  } finally { await view.close(); configureLocalArtifactLoader(null); }
});

test("yaml documents and fenced yaml blocks render with syntax tokens", async () => {
  const view = await mount();
  try {
    for (const path of ["/teams/highlight.yaml", "/teams/highlight.yml"]) {
      configureLocalArtifactLoader(async () => ({ data: yaml, media_type: "text/plain" }));
      await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`<ds-document src="${path}" />`} /></LocalArtifactScope>);
      const document = view.host.querySelector('[data-testid="chat-ds-document"]');
      assert.equal(document?.querySelector("[data-code-language]")?.getAttribute("data-code-language"), "yaml", path);
      assert.ok(document?.querySelector(".token.atrule"), `${path} keys are highlighted`);
      await view.render(null);
    }
    await view.render(<RichMarkdown text={"```yaml\n" + yaml + "```"} />);
    assert.ok(view.host.querySelector('[data-code-language="yaml"] .token.atrule'), "fenced yaml keys are highlighted");
  } finally { await view.close(); configureLocalArtifactLoader(null); }
});

test("preview memory evicts the least recently used file by count and size", () => {
  const file = (data: string) => ({ data, media_type: "text/plain" });
  const memory = createLocalArtifactMemory(2, 10);
  memory.set("a", file("aaa"));
  memory.set("b", file("bbb"));
  assert.ok(memory.get("a"));
  memory.set("c", file("ccc"));
  assert.equal(memory.get("b"), null, "the least recently read entry leaves first");
  memory.set("d", file("dddddddd"));
  assert.deepEqual([memory.get("a"), memory.get("c")], [null, null], "size bound evicts older entries");
  memory.set("huge", file("x".repeat(11)));
  assert.equal(memory.get("huge"), null, "a file above the whole bound is never kept");
  assert.ok(memory.get("d"));
});
