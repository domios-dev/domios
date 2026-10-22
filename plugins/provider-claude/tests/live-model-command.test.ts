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
assert.equal(commands.model.confirmationPromptKind, "switch_confirmation");
assert.equal(commands.effort.confirmationPromptKind, "switch_confirmation");
assert.equal(commands.duringTurn, true);
assert.deepEqual(commands.model.prompt.responseKeys, ["\r"]);
for (const file of ["busy-sonnet-confirmation.txt"]) {
  const screen = fixture(file);
  assert.ok(pattern(commands.model.prompt.screenRegex, "sonnet").test(screen));
  assert.equal(pattern(commands.model.prompt.screenRegex, "foreign").test(screen), false);
  assert.ok(pattern(commands.model.prompt.screenRegex, "sonnet").test(`prefix\n${screen}\nfooter`));
}
const busy = fs.readFileSync(path.join(root, "tests/fixtures/tab-state/normal-working.txt"), "utf8");
assert.equal(pattern(commands.model.prompt.screenRegex, "sonnet").test(busy), false);
assert.equal(pattern(modelPicker.screenRegex, "sonnet").test(busy), false);
assert.ok(pattern(commands.model.confirmation, "sonnet").test("Set model to `Sonnet 5.5` for this session only"));
assert.equal(pattern(commands.model.confirmation, "sonnet").test("Set model to `Sonnet 5.5` and saved as your default for new sessions"), false);

for (const screen of [modelScreen.replaceAll("\r\n", "\n"), modelScreen.replaceAll("\r\n", "\n").replaceAll("\n", "\r\n")]) {
  for (const [model, displayName] of [["opus", "Opus 5.5"], ["opaque-id", "Opus 5.5"], ["opus", "Custom label"]]) {
    assert.equal(pattern(modelPicker.targetRegex, model, "", displayName).exec(screen)?.groups?.position, "2");
  }
}

assert.equal(commands.effort.command.replace("{effort}", "high"), "/effort high");
assert.equal(commands.effort.picker, undefined);
assert.deepEqual(commands.effort.prompt.responseKeys, ["\r"]);
const effortPrompt = fixture("busy-high-effort-confirmation.txt");
for (const screen of [effortPrompt, `prefix\n${effortPrompt}\nseparator\nstatus\n❯ `]) {
  assert.ok(pattern(commands.effort.prompt.screenRegex, "sonnet", "high").test(screen));
  assert.equal(pattern(commands.effort.prompt.screenRegex, "sonnet", "low").test(screen), false);
}
assert.equal(pattern(commands.effort.prompt.screenRegex, "sonnet", "high").test(busy), false);
assert.equal(pattern(commands.effort.prompt.screenRegex, "sonnet", "high").test(fixture("busy-effort-screen.txt")), false);
assert.equal(pattern(commands.effort.prompt.screenRegex, "sonnet", "high").test(effortPrompt.replace(/─{3,}/g, "")), false);
for (const output of [
  fixture("effort-direct-confirmation.txt").split("\n").find(line => line.startsWith("Set effort level to high"))!,
  "Set effort level to high (this session only): details",
]) {
  assert.ok(pattern(commands.effort.confirmation, "opus", "high").test(output));
}
assert.equal(pattern(commands.effort.confirmation, "opus", "high")
  .test(fixture("effort-direct-confirmation.txt").split("\n").find(line => line.startsWith("Set effort level to medium"))!), false);
console.log("Live declarations preserve session-only model selection and direct target effort.");

const busyPicker = fixture("model-picker-with-composer.txt");
assert.ok(pattern(modelPicker.screenRegex, "sonnet").test(busyPicker));
assert.equal(pattern(modelPicker.selectedRegex, "sonnet").exec(busyPicker)?.groups?.position, "5");
assert.equal(pattern(modelPicker.targetRegex, "sonnet").exec(busyPicker)?.groups?.position, "4");

for (const file of ["sonnet-confirmation.txt", "cached-sonnet-confirmation.txt"]) {
 assert.equal(pattern(commands.model.prompt.screenRegex,"sonnet").test(fixture(file)+"\n❯ "),false);
}
function checkScreenDeclaration(value: string, location: string) {
  assert.equal(/\\[zZ]/.test(value), false, `${location} anchors to the whole screen`);
  if (/(^|[^\\])\$/.test(value)) {
    assert.ok(/^\(\?[a-z]*m[a-z]*\)/.test(value), `${location} uses a non-multiline end anchor`);
    assert.equal(value.includes("(?-m"), false, `${location} disables multiline anchors`);
  }
}
function auditScreenDeclarations(value: unknown, location: string) {
  if (typeof value !== "object" || value === null) return;
  for (const [key, child] of Object.entries(value)) {
    if ((key === "screenRegex" || key === "screen_regex") && typeof child === "string") checkScreenDeclaration(child, `${location}.${key}`);
    else auditScreenDeclarations(child, `${location}.${key}`);
  }
}
for (const entry of fs.readdirSync(path.resolve(root, ".."), {withFileTypes: true})) {
  const manifestPath = path.resolve(root, "..", entry.name, "plugin.json");
  if (entry.isDirectory() && fs.existsSync(manifestPath)) auditScreenDeclarations(JSON.parse(fs.readFileSync(manifestPath, "utf8")), entry.name);
}
assert.throws(() => checkScreenDeclaration("^dialog\\z", "fixture"));
assert.throws(() => checkScreenDeclaration("^dialog$", "fixture"));
assert.equal(pattern(commands.model.prompt.screenRegex, "sonnet").test(fixture("idle-model-confirmed.txt")), false);
assert.equal(pattern(modelPicker.screenRegex, "sonnet").test(fixture("busy-effort-screen.txt")), false);
