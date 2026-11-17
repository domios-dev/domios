import assert from "node:assert/strict";
import { referenceMarkerText } from "../../../packages/chat-ui/src/codeterm/referenceMarkers";
import { canonicalMentionSlice, MentionTokenModel, mentionArrowTarget, mentionDeletionRange, normalizePastedMentions, projectMentionDraft, snapMentionSelection } from "./composerMentionTokens";

const marker = '<domios id="tab-1" type="tab" />';
let title = "Elgar";
const model = new MentionTokenModel(`Ask ${marker} now`, () => title);
assert.equal(model.current.text, "Ask @Elgar now");
assert.equal(model.current.canonical, `Ask ${marker} now`);
const tokens = model.current.tokens;
assert.deepEqual(snapMentionSelection(tokens, 5, 5), { start: 4, end: 4 });
assert.deepEqual(snapMentionSelection(tokens, 8, 8), { start: 10, end: 10 });
assert.deepEqual(snapMentionSelection(tokens, 6, 8), { start: 4, end: 10 });
assert.deepEqual(snapMentionSelection(tokens, 0, 7), { start: 0, end: 10 });
assert.equal(mentionArrowTarget(tokens, 4, "ArrowRight"), 10);
assert.equal(mentionArrowTarget(tokens, 10, "ArrowLeft"), 4);
assert.deepEqual(mentionDeletionRange(tokens, 10, 10, "Backspace"), { start: 4, end: 10 });
assert.deepEqual(mentionDeletionRange(tokens, 4, 4, "Delete"), { start: 4, end: 10 });
assert.deepEqual(mentionDeletionRange(tokens, 11, 11, "Backspace", model.current.text), { start: 4, end: 10 });
assert.equal(mentionDeletionRange(tokens, 14, 14, "Backspace", model.current.text), null);
assert.equal(canonicalMentionSlice(model.current, 4, 10), marker);
assert.equal(canonicalMentionSlice(model.current, 0, 14), `Ask ${marker} now`);

model.prepare(4, 10, "");
assert.equal(model.change("Ask  now").canonical, "Ask  now");
assert.equal(model.change("Ask @Elgar now", "historyUndo").canonical, `Ask ${marker} now`);
assert.equal(model.change("Ask  now", "historyRedo").canonical, "Ask  now");
model.change("Ask @Elgar now", "historyUndo");
assert.equal(model.change("Please ask @Elgar now").canonical, `Please ask ${marker} now`);
assert.equal(model.change("Please ask @Elgar later").canonical, `Please ask ${marker} later`);
title = "Renamed";
assert.equal(model.sync(model.current.canonical).text, "Please ask @Elgar later");
model.prepare(11, 17, "Anna");
assert.equal(model.change("Please ask Anna later").canonical, "Please ask Anna later");
assert.equal(model.change("Please ask @Elgar later", "historyUndo").canonical, `Please ask ${marker} later`);

const pasted = normalizePastedMentions("See @Old (tab tab-1) and (tab tab-2)");
assert.equal(pasted, `See ${referenceMarkerText("tab", "tab-1")} and ${referenceMarkerText("tab", "tab-2")}`);
assert.equal(projectMentionDraft(pasted, id => id === "tab-1" ? "Elgar" : "Iris").text, "See @Elgar and @Iris");
assert.equal(projectMentionDraft(`Example: \`${marker}\``, () => "Elgar").tokens.length, 0);

const sameNames = new MentionTokenModel("", () => "Twin");
sameNames.prepare(0, 0, referenceMarkerText("tab", "first")); sameNames.change("@Twin");
sameNames.prepare(0, 5, ""); sameNames.change("");
sameNames.prepare(0, 0, referenceMarkerText("tab", "second")); sameNames.change("@Twin");
sameNames.change("", "historyUndo");
assert.equal(sameNames.change("@Twin", "historyUndo").canonical, referenceMarkerText("tab", "first"));
sameNames.change("", "historyRedo");
assert.equal(sameNames.change("@Twin", "historyRedo").canonical, referenceMarkerText("tab", "second"));

let liveTitle = "Old";
const renamed = new MentionTokenModel(`${marker} `, () => liveTitle);
liveTitle = "New";
renamed.prepare(5, 5, `${marker} `, { id: "tab-1", title: "New" });
assert.equal(renamed.change("@Old @New ").text, "@Old @New ", "new insert freezes its own title independently");
renamed.prepare(5, 9, ""); renamed.change("@Old  ");
assert.equal(renamed.change("@Old @New ", "historyUndo").text, "@Old @New ");
console.log("Mention token projection, atomic ranges, clipboard slices, paste, rename and undo/redo identity checks passed");
