import { describe, expect, it } from "vitest";
import { crc32, folderEntryPath, storedZip } from "./storedZip";

const bytes = (text: string) => new TextEncoder().encode(text);

describe("a browser folder pick packed for the ordinary .team import", () => {
  it("keeps paths relative to the picked folder", () => {
    expect(folderEntryPath("research-desk/team.yaml")).toBe("team.yaml");
    expect(folderEntryPath("research-desk/skills/source-check/SKILL.md")).toBe("skills/source-check/SKILL.md");
    expect(folderEntryPath("team.yaml")).toBe("team.yaml");
  });

  it("writes a stored ZIP whose directory names every entry with its checksum and offset", () => {
    expect(crc32(bytes("abc"))).toBe(0x352441c2);
    const zip = storedZip([{ path: "team.yaml", data: bytes("name: x\n") }, { path: "a/b.md", data: bytes("hi") }]);
    const view = new DataView(zip.buffer);
    const end = zip.length - 22;
    expect(view.getUint32(end, true)).toBe(0x06054b50);
    expect(view.getUint16(end + 10, true)).toBe(2);
    let central = view.getUint32(end + 16, true);
    const names: string[] = [];
    for (let index = 0; index < 2; index += 1) {
      expect(view.getUint32(central, true)).toBe(0x02014b50);
      const nameLength = view.getUint16(central + 28, true);
      const name = new TextDecoder().decode(zip.subarray(central + 46, central + 46 + nameLength));
      const local = view.getUint32(central + 42, true);
      expect(view.getUint32(local, true)).toBe(0x04034b50);
      const size = view.getUint32(local + 18, true);
      const data = zip.subarray(local + 30 + nameLength, local + 30 + nameLength + size);
      expect(view.getUint32(central + 16, true)).toBe(crc32(data));
      names.push(name);
      central += 46 + nameLength;
    }
    expect(names).toEqual(["team.yaml", "a/b.md"]);
  });
});
