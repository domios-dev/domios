// Run: npx tsx src/components/Surface/PointAndAsk.test.tsx

import "./surfaceDomGlobals.testdata";
import { resetCaptureOwnershipForTest } from "../Terminal/voice/larkCaptureModel";
import assert from "node:assert/strict";
import { act } from "react";
import { PointAndAsk, SNIP_IMAGE_LABEL, type AskSubject } from "./PointAndAsk";
import { askDialogLabel, askPlaceholder, elementCard, SNIP_DIALOG_LABEL, snipPlaceholder } from "./surfaceChromeModel";
import { changeValue, getByRole, queryByRole, render, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("PointAndAsk");
const selector = "html > body > main > header > a.brand > span.brand-mark";
const card = elementCard({
  id: "g1",
  label: "link \"snake.\"",
  target: { tagName: "a", selector, react: "Header", text: "snake.", accessibility: { role: "link", name: "snake." } },
});
const path = `Header · ${selector}`;

async function renderAsk(sending = false, subject: AskSubject = { kind: "element", card }) {
  const sends: string[] = [];
  let cancels = 0;
  const stage = document.createElement("div");
  stage.className = "ct-surface-stage";
  const canvas = document.createElement("canvas");
  canvas.className = "ct-surface-canvas";
  let canvasClicks = 0;
  canvas.addEventListener("click", () => { canvasClicks += 1; });
  stage.appendChild(canvas);
  document.body.appendChild(stage);
  const view = await render(
    <PointAndAsk owner="Jev" subject={subject} position={{ left: 10, top: 10 }} sending={sending} onSend={(comment) => sends.push(comment)} onCancel={() => { cancels += 1; }} />,
  );
  stage.appendChild(view.host);
  const dialog = getByRole(view.host, "dialog", subject.kind === "element" ? askDialogLabel(card) : SNIP_DIALOG_LABEL);
  const editor = getByRole(dialog, "textbox", "Your question") as HTMLTextAreaElement;
  const type = (value: string) => changeValue(editor as unknown as HTMLInputElement, value, "input");
  const key = (init: KeyboardEventInit) => act(async () => { editor.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init })); });
  const unmount = async () => { await view.unmount(); stage.remove(); };
  return { dialog, editor, canvas, type, key, sends, cancels: () => cancels, canvasClicks: () => canvasClicks, unmount };
}

const byTestId = (scope: ParentNode, id: string) => scope.querySelector<HTMLButtonElement>(`[data-testid='${id}']`);

function visibleText(element: HTMLElement): string {
  const clone = element.cloneNode(true) as HTMLElement;
  for (const hidden of clone.querySelectorAll("[hidden]")) hidden.remove();
  return clone.textContent ?? "";
}

suite.test("the capsule shows a friendly chip and keeps the CSS path in the tooltip only", async () => {
  const { dialog, editor, unmount } = await renderAsk();
  assert.equal(card.name, "Link «snake.»");
  const chip = dialog.querySelector<HTMLElement>(".ct-surface-ask__chip");
  assert.equal(chip?.textContent, "Link «snake.»");
  assert.equal(chip?.getAttribute("title"), path);
  assert.ok(!visibleText(dialog).includes("html > body"), "the CSS path is never rendered as text");
  assert.equal(document.getElementById(dialog.getAttribute("aria-describedby") ?? "")?.textContent, path, "the path describes the dialog");
  assert.equal(editor.placeholder, askPlaceholder("Jev"));
  assert.equal(queryByRole(dialog, "tablist"), null, "no intent control");
  assert.equal(queryByRole(dialog, "button", "Cancel"), null, "no Cancel button");
  assert.equal(byTestId(dialog, "composer-attach-button"), null, "grab payloads cannot carry attachments yet");
  await unmount();
});

suite.test("an empty draft offers the mic; text turns it into an enabled send", async () => {
  const { dialog, type, sends, unmount } = await renderAsk();
  assert.equal(byTestId(dialog, "composer-send-button"), null);
  assert.ok(byTestId(dialog, "composer-voice-button"), "the chat composer mic is available");
  await type("  Why is this grey?  ");
  const send = byTestId(dialog, "composer-send-button");
  assert.ok(send && !send.disabled);
  await act(async () => { send.dispatchEvent(new PointerEvent("pointerup", { bubbles: true })); });
  assert.deepEqual(sends, ["Why is this grey?"]);
  await unmount();
});

suite.test("Enter sends, Shift+Enter and an empty Enter do not", async () => {
  const { key, type, sends, unmount } = await renderAsk();
  await key({ key: "Enter" });
  assert.deepEqual(sends, [], "nothing to send");
  await type("rename this");
  await key({ key: "Enter", shiftKey: true });
  assert.deepEqual(sends, [], "Shift+Enter is a newline");
  await key({ key: "Enter" });
  assert.deepEqual(sends, ["rename this"]);
  await unmount();
});

suite.test("Escape cancels even with a draft", async () => {
  const { key, type, cancels, unmount } = await renderAsk();
  await type("draft");
  await key({ key: "Escape" });
  assert.equal(cancels(), 1);
  await unmount();
});

suite.test("an outside click cancels only an empty draft and never reaches the canvas", async () => {
  const { canvas, type, cancels, canvasClicks, unmount } = await renderAsk();
  await type("keep me");
  await act(async () => { canvas.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
  await act(async () => { document.body.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
  assert.equal(cancels(), 0, "a draft survives outside clicks");
  await type("");
  await act(async () => { canvas.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
  assert.equal(cancels(), 1, "an empty draft cancels");
  assert.equal(canvasClicks(), 0, "the click neither re-picks nor reports watch-only");
  await unmount();
});

suite.test("sending shows the busy send button and ignores Enter", async () => {
  const { dialog, key, type, sends, unmount } = await renderAsk(true);
  await type("again");
  assert.equal(dialog.getAttribute("data-sending"), "true");
  assert.equal(dialog.getAttribute("aria-busy"), "true");
  assert.ok(byTestId(dialog, "composer-send-button")?.disabled);
  await key({ key: "Enter" });
  assert.deepEqual(sends, []);
  await unmount();
});

suite.test("an area shows the snip as a removable screenshot chip with its context line", async () => {
  let removed = 0;
  const context = "Area 420×180 at (312, 96) · Cart · https://shop.test/cart";
  const subject: AskSubject = { kind: "area", context, image: { thumbnail: "data:image/jpeg;base64,AA==" }, onRemoveImage: () => { removed += 1; } };
  const { dialog, editor, unmount } = await renderAsk(false, subject);
  const chip = byTestId(dialog, "attachment-chip");
  assert.ok(chip, "the snip sits in the input like a chat attachment");
  assert.equal(chip.textContent, SNIP_IMAGE_LABEL);
  assert.equal(chip.querySelector("img")?.getAttribute("src"), "data:image/jpeg;base64,AA==", "the chip shows the crop thumbnail");
  assert.equal(document.getElementById(dialog.getAttribute("aria-describedby") ?? "")?.textContent, context);
  assert.equal(editor.placeholder, snipPlaceholder("Jev"));
  await act(async () => getByRole(chip, "button", `Remove ${SNIP_IMAGE_LABEL}`).click());
  assert.equal(removed, 1);
  await unmount();
});

suite.test("an area without its image keeps only the context line", async () => {
  const subject: AskSubject = { kind: "area", context: "Area 10×10 at (0, 0)", image: null, onRemoveImage: () => undefined };
  const { dialog, unmount } = await renderAsk(false, subject);
  assert.equal(byTestId(dialog, "attachment-chip"), null);
  assert.ok(visibleText(dialog).includes("Area 10×10 at (0, 0)"));
  await unmount();
});

// The composer mic keeps a 30s warm-stream timer that would hold the process open.
void suite.run().then(resetCaptureOwnershipForTest);
