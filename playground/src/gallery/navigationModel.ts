import type { Specimen } from "./types";

const CATEGORY_ORDER = ["Chrome", "Buttons", "Controls", "Menus", "Chat", "Composer", "Voice", "Status", "Automation", "Plans", "Settings", "Sidebar"];

export function groupSpecimens(specimens: Specimen[], query = "") {
  const search = query.trim().toLowerCase();
  const categories = [...new Set([...CATEGORY_ORDER, ...specimens.map((specimen) => specimen.category)])];
  return categories.map((category) => ({
    category,
    entries: specimens.filter((specimen) => specimen.category === category && `${specimen.title} ${category}`.toLowerCase().includes(search)),
  })).filter((group) => group.entries.length);
}
