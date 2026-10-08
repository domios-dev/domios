// Run: npx tsx src/components/Surface/PointAndAsk.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { PointAndAsk } from "./PointAndAsk";
import { askPlaceholder, elementCard, type AskIntent } from "./surfaceChromeModel";
import { getByRole, render, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("PointAndAsk");
const card = elementCard({ id: "g1", label: "button Sign in", target: { react: "LoginForm", selector: "form > button" } });

async function renderAsk() {
  const sends: Array<[string, AskIntent]> = [];
  let cancels = 0;
  const view = await render(
    <PointAndAsk owner="Jev" card={card} position={{ left: 10, top: 10 }} onSend={(comment, intent) => sends.push([comment, intent])} onCancel={() => { cancels += 1; }} />,
  );
  const dialog = getByRole(view.host, "dialog", "Ask about element");
  return { ...view, dialog, sends, cancels: () => cancels };
}

suite.test("the dialog shows the element card and the owner placeholder", async () => {
  const { dialog, unmount } = await renderAsk();
  assert.ok(dialog.textContent?.includes(card.label), "element label from the model is shown");
  assert.ok(dialog.textContent?.includes(card.meta), "element meta from the model is shown");
  const comment = getByRole(dialog, "textbox", "Your question or request") as HTMLTextAreaElement;
  assert.equal(comment.placeholder, askPlaceholder("Jev"));
  assert.ok(getByRole(dialog, "tablist", "Intent"));
  await unmount();
});

suite.test("Send fires once with the comment and the chosen intent", async () => {
  const { dialog, sends, unmount } = await renderAsk();
  (getByRole(dialog, "textbox", "Your question or request") as HTMLTextAreaElement).value = "Why is this grey?";
  await act(async () => getByRole(dialog, "tab", "Bug").click());
  await act(async () => getByRole(dialog, "button", "Send").click());
  assert.deepEqual(sends, [["Why is this grey?", "bug"]]);
  await unmount();
});

suite.test("Ctrl+Enter sends and Escape or Cancel cancels", async () => {
  const { dialog, sends, cancels, unmount } = await renderAsk();
  const comment = getByRole(dialog, "textbox", "Your question or request");
  await act(async () => { comment.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true })); });
  assert.deepEqual(sends, [["", "question"]]);
  await act(async () => { comment.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  assert.equal(cancels(), 1);
  await act(async () => getByRole(dialog, "button", "Cancel").click());
  assert.equal(cancels(), 2);
  await unmount();
});

void suite.run();
