import { expect, it } from "vitest";
import type { NeighborPermission } from "./neighborRuntimeClient";
import { scopeConflicts } from "./neighborScopeConflicts";

const base: NeighborPermission = { allowed: true, scope: "", allow_tab_requests: false, allow_memory: true, allow_files: true, machine_access: "read_only", folder: null };
const grants = (scope: string, patch: Partial<NeighborPermission> = {}) => scopeConflicts({ ...base, scope, ...patch }).map(conflict => conflict.grant);

it("flags a scope that claims memory, files or tabs the switches do not grant", () => {
  expect(grants("They may read my memory about the build.", { allow_memory: false })).toEqual(["memory"]);
  expect(grants("Share the release notes files.", { allow_files: false })).toEqual(["files"]);
  expect(grants("They can open a tab and run commands.")).toEqual(["tabs"]);
  expect(grants("Можно читать мою память о проекте.", { allow_memory: false })).toEqual(["memory"]);
});

it("reads a negated mention as agreeing with the switch", () => {
  expect(grants("Answer questions about the build, but never read my memory.", { allow_memory: false })).toEqual([]);
  expect(grants("No files. Only answer questions.", { allow_files: false })).toEqual([]);
  expect(grants("Do not open tabs.")).toEqual([]);
  expect(grants("Не читать память.", { allow_memory: false })).toEqual([]);
});

it("stays quiet when the switch grants what the scope mentions", () => {
  expect(grants("They may read my memory and files, and open tabs.", { allow_memory: true, allow_files: true, allow_tab_requests: true })).toEqual([]);
});

it("flags changes asked for under read-only access, but not changes to prose", () => {
  expect(grants("Fix the failing tests in the repo.")).toEqual(["writes"]);
  expect(grants("Fix the failing tests in the repo.", { machine_access: "full" })).toEqual([]);
  expect(grants("Write a short summary of the release.")).toEqual([]);
});

it("names a path outside the granted folder, and accepts the folder and paths inside it", () => {
  const folder = { machine_access: "folder" as const, folder: "C:\\work\\domios" };
  expect(scopeConflicts({ ...base, ...folder, scope: "Edit files in C:\\work\\other\\app." }).map(conflict => conflict.line))
    .toEqual(["The scope names C:\\work\\other\\app outside the allowed folder."]);
  expect(grants("Edit c:/Work/Domios/src and C:\\work\\domios.", folder)).toEqual([]);
  expect(grants("See https://example.com/docs and/or the wiki.", folder)).toEqual([]);
  expect(grants("Touch /etc/hosts and ~/secrets.", { machine_access: "folder", folder: "/home/me/project" })).toEqual(["folder"]);
});

it("says nothing while requests are off or the scope is empty", () => {
  expect(grants("Read my memory.", { allowed: false, allow_memory: false })).toEqual([]);
  expect(grants("   ", { allow_memory: false })).toEqual([]);
});
