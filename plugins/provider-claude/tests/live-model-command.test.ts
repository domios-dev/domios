import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifest = JSON.parse(fs.readFileSync(path.join(root, "plugin.json"), "utf8"));
const commands = manifest.commands.modelSwitchCommands;
const fixture = (name: string) => fs.readFileSync(path.join(root, "tests/fixtures/model-switch", name), "utf8");

function pattern(template: string, model: string, effort = "", displayName?: string) {
  const flags = /^\(\?([im]+)\)/.exec(template)?.[1] ?? "";
  return new RegExp(template.replace(/^\(\?[im]+\)/, "")
    .replaceAll("{model}", model).replaceAll("{display_name}", displayName ?? manifest.models.find((entry: { id: string }) => entry.id === model)?.displayName ?? model)
    .replaceAll("{effort}", effort).replaceAll("(?P<", "(?<")
    .replaceAll("\\A", "^").replaceAll("\\z", "$(?![\\s\\S])"), flags);
}

const modelScreen = fixture("model-picker.txt");
const modelPicker = commands.model.picker;
assert.equal(commands.model.command, "/model");
assert.equal(modelPicker.responseKey, "s");
assert.ok(pattern(modelPicker.screenRegex, "opus").test(modelScreen));
assert.equal(pattern(modelPicker.selectedRegex, "opus").exec(modelScreen)?.groups?.position, "4");
assert.equal(pattern(modelPicker.targetRegex, "opus").exec(modelScreen)?.groups?.position, "2");
assert.equal(pattern(modelPicker.targetRegex, "haiku").exec(modelScreen)?.groups?.position, "5");
assert.equal(pattern(modelPicker.targetRegex, "sonnet").exec(modelScreen)?.groups?.position, "4");
assert.equal(pattern(modelPicker.targetRegex, "sonnet").exec(modelScreen)?.groups?.label, "Sonnet 5.5");
assert.equal(pattern(modelPicker.targetRegex, "absent").exec(modelScreen), null);
assert.ok(pattern(commands.model.prompt.screenRegex, "opus").test(fixture("opus-confirmation.txt")));
assert.ok(pattern(commands.model.confirmation, "sonnet").test("Set model to `Sonnet 5.5` for this session only"));
assert.equal(pattern(commands.model.confirmation, "sonnet").test("Set model to `Sonnet 5.5` and saved as your default for new sessions"), false);

for (const screen of [modelScreen.replaceAll("\r\n", "\n"), modelScreen.replaceAll("\r\n", "\n").replaceAll("\n", "\r\n")]) {
  for (const [model, displayName] of [["opus", "Opus 5.5"], ["opaque-id", "Opus 5.5"], ["opus", "Custom label"]]) {
    assert.equal(pattern(modelPicker.targetRegex, model, "", displayName).exec(screen)?.groups?.position, "2");
  }
}

assert.equal(commands.effort.command.replace("{effort}", "high"), "/effort high");
assert.equal(commands.effort.picker, undefined);
assert.equal(commands.effort.prompt, undefined);
for (const output of [
  fixture("effort-direct-confirmation.txt").split("\n").find(line => line.startsWith("Set effort level to high"))!,
  "Set effort level to high (this session only): details",
]) {
  assert.ok(pattern(commands.effort.confirmation, "opus", "high").test(output));
}
assert.equal(pattern(commands.effort.confirmation, "opus", "high")
  .test(fixture("effort-direct-confirmation.txt").split("\n").find(line => line.startsWith("Set effort level to medium"))!), false);
console.log("Live declarations preserve session-only model selection and direct target effort.");
