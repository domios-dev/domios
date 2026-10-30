import assert from "node:assert/strict";
import { localArtifactTextParts, parseLocalArtifactDirective } from "./localArtifactDirective";
for (const prefix of ["ds"]) {
  for (const kind of ["image", "page", "document"]) {
    const path = "C:\\work\\a_b_c [1].png";
    assert.deepEqual(parseLocalArtifactDirective(`@${prefix}-${kind} "${path}"`), { kind, path });
    const source = `Lead\r\n@${prefix}-${kind} "${path}"\r\nTail`;
    const parts = localArtifactTextParts(source);
    assert.equal(parts.map((part) => part.text).join(""), source);
    assert.deepEqual(parts.map((part) => part.directive).filter(Boolean), [{ kind, path }]);
  }
}
for (const source of ["```md\n@ds-image /a.png\n```", "~~~~\n@ct-page /a.html\n~~~\n@ds-document /a.md\n~~~~", "    @ds-image /a.png", "\t@ds-image /a.png", "Text @ds-image /a.png"]) {
  assert.equal(localArtifactTextParts(source).some((part) => part.directive), false);
}
assert.equal(parseLocalArtifactDirective("@ds-image /a.png\n@ds-image /b.png"), null);
assert.equal(parseLocalArtifactDirective("@ct-document /b.md"), null);
assert.equal(localArtifactTextParts("@ds-image /a.png\n@ct-document /b.md").filter((part) => part.directive).length, 1);
console.log("localArtifactDirective: current parser and segmentation cases passed");

assert.deepEqual(parseLocalArtifactDirective('@ds-image "/a one.png" "/b two.png"'), { kind: "image", path: "/a one.png", paths: ["/a one.png", "/b two.png"] });
assert.deepEqual(parseLocalArtifactDirective("@ds-image /a.png /b.png"), { kind: "image", path: "/a.png", paths: ["/a.png", "/b.png"] });
assert.deepEqual(parseLocalArtifactDirective("@ds-image C:/a.png D:/b.png"), { kind: "image", path: "C:/a.png", paths: ["C:/a.png", "D:/b.png"] });

assert.deepEqual(parseLocalArtifactDirective('@ds-image "/a one.png" /b.png'), { kind: "image", path: "/a one.png", paths: ["/a one.png", "/b.png"] });

assert.deepEqual(parseLocalArtifactDirective("@ds-image /work/owner's image.png"), { kind: "image", path: "/work/owner's image.png" });

assert.deepEqual(parseLocalArtifactDirective("@ds-image /owner's.png /two.png"), { kind: "image", path: "/owner's.png", paths: ["/owner's.png", "/two.png"] });
