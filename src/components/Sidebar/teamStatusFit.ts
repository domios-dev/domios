export function compactTeamStatus(fullWidth: number, availableWidth: number): boolean {
  return Number.isFinite(fullWidth) && Number.isFinite(availableWidth)
    && availableWidth > 0 && fullWidth > availableWidth + 0.5;
}
