import assert from "node:assert/strict";
import test from "node:test";
import { localArtifactTextParts, parseLocalArtifactDirective } from "./localArtifactDirective";
import { serializeDsBlock } from "./dsBlocks";

test("localArtifactDirective projects canonical paths without Markdown or backslash decoding", () => {
  for (const kind of ["image", "page", "document"] as const) {
    for (const path of ["C:\\work\\a_b_c [1].png", "\\\\server\\share\\owner's file", "/work/a > b.png"]) {
      const source = serializeDsBlock(kind === "image" ? { kind, sources: [path] } : { kind, src: path });
      assert.deepEqual(parseLocalArtifactDirective(source), { kind, path });
      const surrounded = `Lead\r\n${source}\r\nTail`;
      const parts = localArtifactTextParts(surrounded);
      assert.equal(parts.map((part) => part.text).join(""), surrounded);
      assert.deepEqual(parts.flatMap((part) => part.directive ?? []), [{ kind, path }]);
    }
  }
});
test("localArtifactDirective gallery and literal contexts share one scanner", () => {
  const paths = ["/a one.png", "D:\\b.png"];
  assert.deepEqual(parseLocalArtifactDirective(serializeDsBlock({ kind: "image", sources: paths })), { kind: "image", path: paths[0], paths });
  const tag = '<ds-image src="/a.png" />';
  for (const source of [`\`\`\`md\n${tag}\n\`\`\``, `~~~~\n${tag}\n~~~\n${tag}\n~~~~`, `    ${tag}`, `\t${tag}`, `Text ${tag}`, `> ${tag}`, '@ds-image /a.png', '@ct-document /b.md']) assert.equal(localArtifactTextParts(source).some((part) => part.directive), false, source);
  assert.equal(parseLocalArtifactDirective(`${tag}\n${tag}`), null);
  assert.equal(parseLocalArtifactDirective('<ds-page src="relative.html" />'), null);
});
