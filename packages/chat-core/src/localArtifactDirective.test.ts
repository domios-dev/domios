import assert from "node:assert/strict";
import { localArtifactTextParts, parseLocalArtifactDirective } from "./localArtifactDirective";
for (const prefix of ["ds", "ct"]) {
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
assert.equal(localArtifactTextParts("@ds-image /a.png\n@ct-document /b.md").filter((part) => part.directive).length, 2);
console.log("localArtifactDirective: 14 parser and segmentation cases passed");
