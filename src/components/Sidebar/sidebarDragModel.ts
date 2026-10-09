/** One draggable top-level unit of a workspace: a loose tab or a whole team block. */
export interface DragUnit {
  key: string;
  tabIds: string[];
}

/**
 * The backend reorders top-level tabs as one full list, so every drag is expanded
 * into the complete workspace order: units in their new order, then any top-level
 * tab no unit owns (hidden or attached), keeping its relative place.
 */
export function workspaceTabOrder(topLevelIds: readonly string[], units: readonly DragUnit[]): string[] {
  const topLevel = new Set(topLevelIds);
  const emitted = new Set<string>();
  const order: string[] = [];
  for (const unit of units) {
    for (const id of unit.tabIds) {
      if (!topLevel.has(id) || emitted.has(id)) continue;
      emitted.add(id);
      order.push(id);
    }
  }
  for (const id of topLevelIds) if (!emitted.has(id)) order.push(id);
  return order;
}

/** Units reordered to `orderedKeys`; keys missing from it keep their trailing order. */
export function unitsInOrder(units: readonly DragUnit[], orderedKeys: readonly string[]): DragUnit[] {
  const byKey = new Map(units.map((unit) => [unit.key, unit]));
  const ordered = orderedKeys.map((key) => byKey.get(key)).filter((unit): unit is DragUnit => unit != null);
  const seen = new Set(ordered.map((unit) => unit.key));
  return [...ordered, ...units.filter((unit) => !seen.has(unit.key))];
}

/** The team unit with one of its sections (managers, workers, terminals) reordered. */
export function withTeamSection(
  units: readonly DragUnit[],
  teamKey: string,
  sections: readonly (readonly string[])[],
  sectionIndex: number,
  orderedSection: readonly string[],
): DragUnit[] {
  const nextSections = sections.map((section, index) => (index === sectionIndex ? [...orderedSection] : [...section]));
  return units.map((unit) => (unit.key === teamKey ? { key: unit.key, tabIds: nextSections.flat() } : unit));
}
