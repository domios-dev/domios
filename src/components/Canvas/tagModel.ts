import { MILESTONE_LANE_COLORS } from "./settings";
import type { TagFacet } from "./types";

export interface FacetChip {
  tag: string;
  key: string;
  value: string;
  label: string;
  color: string | null;
  primary: boolean;
}

export interface ResolvedTags {
  primary: FacetChip | null;
  facets: FacetChip[];
  topics: string[];
}

export function isFacetedTag(tag: string): boolean {
  const i = tag.indexOf(":");
  return i > 0 && i < tag.length - 1;
}

function facetKey(tag: string): string | null {
  if (!isFacetedTag(tag)) return null;
  return tag.slice(0, tag.indexOf(":"));
}

function slug(raw: string): string {
  let out = "";
  let dash = false;
  for (const ch of raw.toLowerCase()) {
    if (/[a-z0-9]/.test(ch)) {
      out += ch;
      dash = false;
    } else if (!dash && out.length > 0) {
      out += "-";
      dash = true;
    }
  }
  return out.replace(/^-+|-+$/g, "");
}

/** Mirrors plan tag_model handle_seed for UI previews. */
export function previewHandle(tags: string[] | undefined, title: string): string {
  const clean = tags?.map((t) => t.trim()).filter(Boolean) ?? [];
  const primary = clean.find(isFacetedTag)?.split(":", 2)[1];
  const topic = clean.find((tag) => !isFacetedTag(tag))
    ?? title.split(/\s+/)[0]
    ?? "item";
  return primary ? `${slug(primary)}-${slug(topic)}` : slug(title);
}

export function applyTags(tags: string[], add: string[], primary?: string | null): string[] {
  const next = [...tags];
  if (primary) {
    const key = facetKey(primary);
    if (key) {
      for (let i = next.length - 1; i >= 0; i--) {
        if (facetKey(next[i]) === key) next.splice(i, 1);
      }
    }
    const idx = next.indexOf(primary);
    if (idx >= 0) next.splice(idx, 1);
    next.unshift(primary);
  }
  for (const tag of add) {
    if (primary && tag === primary) continue;
    const key = facetKey(tag);
    if (key) {
      for (let i = next.length - 1; i >= 0; i--) {
        if (facetKey(next[i]) === key) next.splice(i, 1);
      }
    }
    if (!next.includes(tag)) next.push(tag);
  }
  return next;
}

export function facetColorMap(facets: TagFacet[] | undefined): Map<string, string> {
  const map = new Map<string, string>();
  if (!facets?.length) return map;
  let lane = 0;
  for (const facet of facets) {
    for (const value of facet.values) {
      const tag = `${facet.key}:${value.v}`;
      if (map.has(tag)) continue;
      const declared = value.color?.trim() || null;
      const color = declared ?? (facet.color_lane ? MILESTONE_LANE_COLORS[lane % MILESTONE_LANE_COLORS.length] : null);
      if (color) map.set(tag, color);
      if (facet.color_lane && !declared) lane++;
    }
  }
  return map;
}

function facetLabel(facet: TagFacet | undefined, value: string): string {
  const hit = facet?.values.find((v) => v.v === value);
  return hit?.label?.trim() || value.replace(/-/g, " ");
}

export function resolveTags(tags: string[] | undefined, facets: TagFacet[] | undefined): ResolvedTags {
  const clean = tags?.map((t) => t.trim()).filter(Boolean) ?? [];
  const facetByKey = new Map((facets ?? []).map((f) => [f.key, f]));
  const colors = facetColorMap(facets);
  const facetsOut: FacetChip[] = [];
  const topics: string[] = [];
  let primary: FacetChip | null = null;

  for (const tag of clean) {
    if (!isFacetedTag(tag)) {
      topics.push(tag);
      continue;
    }
    const [key, value] = tag.split(":", 2);
    const facet = facetByKey.get(key);
    const chip: FacetChip = {
      tag,
      key,
      value,
      label: facetLabel(facet, value),
      color: colors.get(tag) ?? null,
      primary: false,
    };
    if (!primary) {
      chip.primary = true;
      primary = chip;
    }
    facetsOut.push(chip);
  }

  return { primary, facets: facetsOut, topics };
}

/** Facet key → selected facet tags (e.g. area → {area:nav, area:layout}). */
export type TagFilterSelection = Map<string, Set<string>>;

export function isTagFilterActive(selection: TagFilterSelection): boolean {
  for (const bucket of selection.values()) {
    if (bucket.size > 0) return true;
  }
  return false;
}

export function isTagFilterSelected(selection: TagFilterSelection, tag: string): boolean {
  if (!isFacetedTag(tag)) return false;
  const key = tag.slice(0, tag.indexOf(":"));
  return selection.get(key)?.has(tag) ?? false;
}

export function toggleTagFilter(selection: TagFilterSelection, tag: string): TagFilterSelection {
  if (!isFacetedTag(tag)) return selection;
  const key = tag.slice(0, tag.indexOf(":"));
  const next = new Map(selection);
  const bucket = new Set(next.get(key) ?? []);
  if (bucket.has(tag)) bucket.delete(tag);
  else bucket.add(tag);
  if (bucket.size === 0) next.delete(key);
  else next.set(key, bucket);
  return next;
}

/** OR within a facet, AND across facets with active selections. */
export function taskMatchesTagFilter(tags: string[] | undefined, selection: TagFilterSelection): boolean {
  if (!isTagFilterActive(selection)) return true;
  const faceted = (tags?.map((t) => t.trim()).filter(Boolean) ?? []).filter(isFacetedTag);
  for (const selected of selection.values()) {
    if (selected.size === 0) continue;
    if (!faceted.some((t) => selected.has(t))) return false;
  }
  return true;
}

export function legendEntries(facets: TagFacet[] | undefined): { key: string; label: string; values: { tag: string; label: string; color: string }[] }[] {
  if (!facets?.length) return [];
  const colors = facetColorMap(facets);
  return facets
    .filter((f) => f.color_lane && f.values.length > 0)
    .map((facet) => ({
      key: facet.key,
      label: facet.label?.trim() || facet.key,
      values: facet.values
        .map((v) => {
          const tag = `${facet.key}:${v.v}`;
          const color = colors.get(tag);
          if (!color) return null;
          return { tag, label: v.label?.trim() || v.v.replace(/-/g, " "), color };
        })
        .filter((v): v is { tag: string; label: string; color: string } => v != null),
    }))
    .filter((f) => f.values.length > 0);
}
