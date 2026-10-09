import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const commands = JSON.parse(fs.readFileSync(path.join(root, "plugin.json"), "utf8"))
  .commands.modelSwitchCommands;
const fixture = (name: string) => fs.readFileSync(path.join(root, "tests/fixtures/model-switch", name), "utf8");

function pattern(template: string, model: string, effort = "") {
  const flags = /^\(\?([im]+)\)/.exec(template)?.[1] ?? "";
  return new RegExp(template.replace(/^\(\?[im]+\)/, "")
    .replaceAll("{model}", model).replaceAll("{display_name}", `Claude ${model}`)
    .replaceAll("{effort}", effort).replaceAll("(?P<", "(?<")
    .replaceAll("\\z", "$(?![\\s\\S])"), flags);
}

const modelScreen = fixture("model-picker.txt");
const modelPicker = commands.model.picker;
assert.equal(commands.model.command, "/model");
assert.equal(modelPicker.responseKey, "s");
assert.ok(pattern(modelPicker.screenRegex, "opus").test(modelScreen));
assert.equal(pattern(modelPicker.selectedRegex, "opus").exec(modelScreen)?.groups?.position, "4");
assert.equal(pattern(modelPicker.targetRegex, "opus").exec(modelScreen)?.groups?.position, "2");
assert.equal(pattern(modelPicker.targetRegex, "haiku").exec(modelScreen)?.groups?.position, "5");
assert.equal(pattern(modelPicker.targetRegex, "absent").exec(modelScreen), null);
assert.ok(pattern(commands.model.prompt.screenRegex, "opus").test(fixture("opus-confirmation.txt")));
assert.ok(pattern(commands.model.confirmation, "sonnet").test("Set model to `Sonnet 5.5` for this session only"));
assert.equal(pattern(commands.model.confirmation, "sonnet").test("Set model to `Sonnet 5.5` and saved as your default for new sessions"), false);

const effortScreen = fixture("effort-picker.txt");
const effortPicker = commands.effort.picker;
assert.equal(commands.effort.command, "/effort");
assert.equal(effortPicker.responseKey, "s");
const frame = pattern(effortPicker.screenRegex, "opus").exec(effortScreen)?.[0];
assert.ok(frame);
const cursor = pattern(effortPicker.selectedRegex, "opus").exec(frame)?.groups?.position.length;
const high = pattern(effortPicker.targetRegex, "opus", "high").exec(frame)?.groups;
assert.ok(cursor !== undefined && high);
assert.ok(cursor >= high.position.length && cursor < high.position.length + high.label.length);
const xhigh = pattern(effortPicker.targetRegex, "opus", "xhigh").exec(frame)?.groups;
assert.ok(xhigh && cursor < xhigh.position.length);
assert.ok(pattern(commands.effort.confirmation, "opus", "high")
  .test("Set effort level to high (this session only): details"));
console.log("Live command declarations match captured model, confirmation, and effort screens.");
