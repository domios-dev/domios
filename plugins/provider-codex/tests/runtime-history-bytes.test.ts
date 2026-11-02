import assert from "node:assert/strict";
import { utf8Bytes, serializedBytes, sanitizeHistory } from "../src/runtimeHistoryBounds";

for (const value of ["", "ascii\0\n", "éЖ漢字", "🧪🌍", "\ud800", "\udc00", "\ud800x\udc00", "\ud800\ud800\udc00\udc00"]) {
  assert.equal(utf8Bytes(value), Buffer.byteLength(value), JSON.stringify(value));
}
const bmp = Array.from({ length: 65536 }, (_, point) => String.fromCharCode(point)).join("");
assert.equal(utf8Bytes(bmp), Buffer.byteLength(bmp));
const astral = Array.from({ length: 4096 }, (_, point) => String.fromCodePoint(0x10000 + point * 251)).join("");
assert.equal(utf8Bytes(astral), Buffer.byteLength(astral));
function previousStateBytes(value: string): number {
  let bytes = 0;
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit < 0x80) bytes++;
    else if (unit < 0x800) bytes += 2;
    else if (unit >= 0xd800 && unit < 0xdc00) { bytes += 4; index++; }
    else bytes += 3;
  }
  return bytes;
}
for (const text of [bmp, astral, "\0\n\"\\".repeat(2048), "\ud800x\udc00"]) {
  const serialized = JSON.stringify({agent_result:text});
  assert.equal(utf8Bytes(serialized), previousStateBytes(serialized));
  assert.equal(utf8Bytes(serialized), Buffer.byteLength(serialized));
  const reduction = {state:{agent_result:text},events:[]};
  assert.equal(serializedBytes(reduction), Buffer.byteLength(JSON.stringify(reduction)));
}
for (const character of ["x", "é", "漢", "🧪"]) {
  const limit = 16384 / Buffer.byteLength(character);
  const within = character.repeat(limit);
  assert.equal(sanitizeHistory(within), within);
  assert.notEqual(sanitizeHistory(within + character), within + character);
}
console.log("PASS history UTF-8 budgets: BMP, astral, lone surrogates, exact truncation boundary");
