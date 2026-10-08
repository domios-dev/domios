import assert from "node:assert/strict";
import { documentsFromTranscript } from "../chatDocuments";
import { breadcrumbSegments, buildPathTree, displayPath, isFolder, pathName, shortenPath, treeFocusIndex, treeLeaves, treeParentIndex, visibleTreeRows } from "./pathTree";

const docs = (paths: string[], home: string | null) => buildPathTree(documentsFromTranscript([{ content: paths.map((path) => `@ds-document ${path}`).join("\n") }])
  .map((document) => ({ id: document.path, path: shortenPath(document.path, home), item: document })));

assert.equal(shortenPath("/home/ann/work/a.md", "/home/ann"), "~/work/a.md");
assert.equal(shortenPath("/home/anna/a.md", "/home/ann"), "/home/anna/a.md");
assert.equal(shortenPath("C:\\Users\\Ann\\a.md", "c:/users/ann"), "~/a.md");
assert.equal(shortenPath("/opt/a.md", "/home/ann"), "/opt/a.md");
assert.equal(shortenPath("/home/ann", "/home/ann/"), "~");

assert.equal(displayPath("src/a.ts", { cwd: "/repo", home: "/home/ann" }), "src/a.ts");
assert.equal(displayPath("/repo/src/a.ts", { cwd: "/repo/", home: "/home/ann" }), "src/a.ts");
assert.equal(displayPath("C:\\Repo\\src\\a.ts", { cwd: "c:/repo", home: null }), "src/a.ts");
assert.equal(displayPath("C:\\Users\\Ann\\.codeterm-dev\\state\\x.json", { cwd: "C:/Repo", home: "C:\\Users\\Ann" }), "~/.codeterm-dev/state/x.json");
assert.equal(displayPath("/elsewhere/x.ts", { cwd: "/repo", home: null }), "/elsewhere/x.ts");
assert.equal(pathName("C:\\a\\b.md"), "b.md");

const tree = docs(["/home/ann/work/z.md", "/home/ann/work/b.md", "/home/ann/work/docs/deep/a.md", "/home/ann/work/assets/c.md"], "/home/ann");
assert.equal(tree[0].name, "~/work");
assert.ok(isFolder(tree[0]));
assert.deepEqual(tree[0].children.map((node) => node.name), ["assets", "docs/deep", "b.md", "z.md"]);
assert.equal(treeLeaves(tree[0]).length, 4);
assert.deepEqual(visibleTreeRows(tree, new Set([tree[0].id])).map(({ node }) => node.name), ["~/work"]);
const rows = visibleTreeRows(tree, new Set());
assert.deepEqual(rows.map(({ level }) => level), [1, 2, 3, 2, 3, 2, 2]);
assert.equal(treeParentIndex(rows, 2), 1);
assert.equal(treeParentIndex(rows, 1), 0);
assert.equal(treeParentIndex(rows, 0), -1);
assert.deepEqual(buildPathTree([]), []);
assert.deepEqual(docs(["/a/report.md"], null).map((node) => node.name), ["/a"]);
assert.deepEqual(docs(["C:/work/a.md", "D:/other/b.md"], null).map((node) => node.name), ["C:/work", "D:/other"]);
assert.deepEqual(buildPathTree([{ id: "a", path: "a.ts", item: 1 }, { id: "b", path: "src/b.ts", item: 2 }]).map((node) => node.name), ["src", "a.ts"]);

assert.equal(treeFocusIndex("ArrowDown", 1, 3), 2);
assert.equal(treeFocusIndex("ArrowDown", 2, 3), 2);
assert.equal(treeFocusIndex("ArrowUp", 0, 3), 0);
assert.equal(treeFocusIndex("Home", 2, 3), 0);
assert.equal(treeFocusIndex("End", 0, 3), 2);

assert.deepEqual(breadcrumbSegments("~/work/a.md"), ["~", "work", "a.md"]);
assert.deepEqual(breadcrumbSegments("/a/b/c/d/e.md"), ["/a", "…", "c", "d", "e.md"]);
assert.deepEqual(breadcrumbSegments("C:\\x\\y\\z\\w\\v.md", 2), ["C:", "…", "w", "v.md"]);
console.log("Path tree checks passed");
