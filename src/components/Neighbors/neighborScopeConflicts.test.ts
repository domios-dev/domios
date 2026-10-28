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
  expect(grants("Write a summary of the project.")).toEqual([]);
  expect(grants("Write a summary of the code.")).toEqual([]);
  expect(grants("Create a summary from the files.")).toEqual([]);
  expect(grants("Write code for the parser.")).toEqual(["writes"]);
  expect(grants("Create a new file for the notes.")).toEqual(["writes"]);
  expect(grants("Review the commit history of the repo.")).toEqual([]);
  expect(grants("Обсудить изменения в проекте.")).toEqual([]);
  expect(grants("Исправить ошибки в проекте.")).toEqual(["writes"]);
  expect(grants("Напиши код для парсера.")).toEqual(["writes"]);
});

it("compares paths lexically: dot segments resolve, POSIX keeps case, Windows drives ignore it, and home is never guessed", () => {
  const outsideOf = (folder: string, scope: string) => scopeConflicts({ ...base, machine_access: "folder", folder, scope }).map(conflict => conflict.line);
  expect(outsideOf("/allowed", "Read /allowed/../private/keys.")).toEqual(["The scope names /allowed/../private/keys outside the allowed folder."]);
  expect(outsideOf("/allowed", "Read /allowed/./docs/../notes.")).toEqual([]);
  expect(outsideOf("/home/me/project", "Read /home/me/Project/src.")).toEqual(["The scope names /home/me/Project/src outside the allowed folder."]);
  expect(outsideOf("C:\\Work\\App", "Read c:/work/app/src and C:\\WORK\\APP.")).toEqual([]);
  expect(outsideOf("/home/me/project", "Read ~/project/notes.")).toEqual([]);
  expect(outsideOf("~/project", "Read ~/other.")).toEqual(["The scope names ~/other outside the allowed folder."]);
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
