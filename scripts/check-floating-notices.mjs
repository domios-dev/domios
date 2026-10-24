import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const allowed = new Set([
  "src/components/Notifications/NotificationToast.tsx",
  "src/components/MeshPanel/ServiceDrawer.tsx",
  "src/components/MeshPanel/WebAccessPanel.tsx",
  "src/components/Sidebar/RemoteTabPicker.tsx",
  "src/components/Canvas/missionControl/MissionControlPlanLifecycleDialog.tsx",
]);

export function floatingLiveRegions(source, filename = "fixture.tsx", fixedClasses = new Set()) {
  const file = ts.createSourceFile(filename, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const failures = [];
  function visit(node, floating = false) {
    const opening = ts.isJsxElement(node) ? node.openingElement : ts.isJsxSelfClosingElement(node) ? node : null;
    if (opening) {
      const attributes = opening.attributes.properties.filter(ts.isJsxAttribute);
      const attribute = (name) => attributes.find((item) => item.name.getText(file) === name)?.initializer?.getText(file) ?? "";
      const modal = /["']dialog["']/.test(attribute("role")) && /["']true["']/.test(attribute("aria-modal"));
      const classes = attribute("className");
      if (modal) floating = false;
      else floating ||= /\bfixed\b/.test(classes) || [...fixedClasses].some((name) => classes.includes(name)) || /position\s*:\s*["']fixed["']/.test(attribute("style"));
      if (floating && (attribute("aria-live") || /["'](?:status|alert)["']/.test(attribute("role")))) {
        failures.push(file.getLineAndCharacterOfPosition(opening.getStart(file)).line + 1);
      }
    }
    ts.forEachChild(node, (child) => visit(child, floating));
  }
  visit(file);
  return failures;
}

function filesUnder(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (entry.name === "node_modules" || entry.name === "dist") return [];
    const filename = path.join(directory, entry.name);
    return entry.isDirectory() ? filesUnder(filename) : [filename.replaceAll("\\", "/")];
  });
}

export function checkFloatingNotices() {
  const files = ["src", "packages"].flatMap(filesUnder);
  const fixedClasses = new Set();
  for (const filename of files.filter((name) => name.endsWith(".css"))) {
    const css = fs.readFileSync(filename, "utf8");
    for (const match of css.matchAll(/([^{}]+)\{([^{}]*\bposition\s*:\s*fixed\b[^{}]*)\}/g)) {
      for (const selector of match[1].matchAll(/\.([\w-]+)/g)) fixedClasses.add(selector[1]);
    }
  }
  const failures = [];
  for (const filename of files.filter((name) => /\.[jt]sx$/.test(name) && !/\.(?:test|testdata|testkit)\./.test(name))) {
    if (allowed.has(filename)) continue;
    for (const line of floatingLiveRegions(fs.readFileSync(filename, "utf8"), filename, fixedClasses)) failures.push(`${filename}:${line}`);
  }
  return failures;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const failures = checkFloatingNotices();
  if (failures.length) {
    console.error(`Floating live-region notices must use the notification toast stack:\n${failures.join("\n")}`);
    process.exitCode = 1;
  } else console.log("Floating notice guard passed.");
}
