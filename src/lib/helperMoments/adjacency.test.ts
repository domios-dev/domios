import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { getHelpSectionIds } from "../../help/content";
import { adjacentSuggestion, SECTION_CAPABILITIES, STRONG_CAPABILITY_EDGES, strongNeighbourSections, ADJACENT_SECTIONS } from "./adjacency";
import type { HelperFacts } from "./helperBlocks";

const FACTS: HelperFacts = {
  telegramConnected: false,
  plugins: [
    { id: "telegram-client", name: "Telegram Client", installed: false },
    { id: "teams-client", name: "Teams Client", installed: false },
  ],
  blueprintsInUse: false,
};

describe("capability adjacency", () => {
  it("uses only real direct feeds edges and capability nodes of the essence map", () => {
    const map = readFileSync(resolve(process.cwd(), "essences/map.jsonl"), "utf8")
      .trim().split("\n").map((line) => JSON.parse(line) as { kind: string; id?: string; from?: string; to?: string; type?: string });
    const nodes = new Set(map.filter((row) => row.kind === "node").map((row) => row.id));
    const feeds = new Set(map.filter((row) => row.kind === "edge" && row.type === "feeds").map((row) => `${row.from}>${row.to}`));
    for (const capabilities of Object.values(SECTION_CAPABILITIES)) {
      for (const capability of capabilities) expect(nodes.has(`capability:${capability}`), capability).toBe(true);
    }
    for (const [from, to] of STRONG_CAPABILITY_EDGES) expect(feeds.has(`capability:${from}>capability:${to}`), `${from} -> ${to}`).toBe(true);
  });

  it("maps only to Guide sections that exist", () => {
    const sections = new Set(getHelpSectionIds("en"));
    for (const id of [...Object.keys(SECTION_CAPABILITIES), ...ADJACENT_SECTIONS.map((entry) => entry.sectionId)]) expect(sections.has(id), id).toBe(true);
  });

  it("finds neighbours through strong edges in either direction", () => {
    expect(strongNeighbourSections([{ kind: "guide", sectionId: "phone" }])).toEqual(new Set(["messengers", "messenger-plugins"]));
    expect(strongNeighbourSections([{ kind: "guide", sectionId: "supporter-agent" }])).toEqual(new Set(["memory", "blueprints"]));
    expect(strongNeighbourSections([{ kind: "guide", sectionId: "tips" }])).toEqual(new Set());
  });

  it("offers one unused neighbour block after an answer that showed a capability", () => {
    expect(adjacentSuggestion("You can reach me from your phone.\n\n@ds-guide phone", FACTS)?.block).toEqual({ kind: "guide", sectionId: "messenger-plugins" });
    expect(adjacentSuggestion("Plugins:\n@ds-plugin telegram-client", FACTS)?.block).toEqual({ kind: "guide", sectionId: "phone" });
  });

  it("offers nothing for plain prose, weak links, used or unknown neighbours, or a block already shown", () => {
    expect(adjacentSuggestion("Telegram works through @ds-guide phone inline.", FACTS)).toBeNull();
    expect(adjacentSuggestion("@ds-guide tips", FACTS)).toBeNull();
    const installed = { ...FACTS, plugins: [{ id: "telegram-client", name: "Telegram Client", installed: true }] };
    expect(adjacentSuggestion("@ds-guide phone", installed)).toBeNull();
    expect(adjacentSuggestion("@ds-guide supporter-agent", { ...FACTS, blueprintsInUse: null })).toBeNull();
    expect(adjacentSuggestion("@ds-guide messenger-plugins\n@ds-guide phone", FACTS)).toBeNull();
  });
});
