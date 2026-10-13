export interface GuideSectionPosition<Id extends string> {
  id: Id;
  top: number;
  bottom: number;
}

export interface GuideViewport {
  top: number;
  bottom: number;
  atEnd: boolean;
}

export function activeGuideSection<Id extends string>(
  sections: readonly GuideSectionPosition<Id>[],
  viewport: GuideViewport,
): Id | null {
  if (viewport.bottom <= viewport.top || sections.length === 0) return null;
  if (viewport.atEnd) return sections[sections.length - 1].id;

  const readingLine = Math.min(viewport.top + 16, viewport.bottom);
  let active: Id | null = null;
  for (const section of sections) {
    if (section.top <= readingLine) active = section.id;
    else break;
  }
  return active ?? sections.find((section) =>
    section.bottom > viewport.top && section.top < viewport.bottom,
  )?.id ?? null;
}
