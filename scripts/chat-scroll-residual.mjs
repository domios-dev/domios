// Shared by the phone decoder, Chromium harness and synthetic regression oracle.
export function sameAnchorLayoutResidual(before, after, bound = 8) {
  if (before.anchor == null || before.anchor === 0 || before.anchor !== after.anchor) return null;
  const residual = after.anchorTop - before.anchorTop + after.scrollTop - before.scrollTop;
  if (!Number.isFinite(residual)) return null;
  return { residual, failed: Math.abs(residual) > bound };
}
