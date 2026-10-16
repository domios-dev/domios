// Shared by the phone decoder, Chromium harness and synthetic regression oracle.
export function sameAnchorLayoutResidual(before, after, bound = 8, compensated = false) {
  if (before.anchor == null || before.anchor === 0 || before.anchor !== after.anchor) return null;
  const residual = after.anchorTop - before.anchorTop + after.scrollTop - before.scrollTop;
  if (!Number.isFinite(residual)) return null;
  const visualDelta = after.anchorTop - before.anchorTop;
  return { residual, visualDelta, compensated, failed: Math.abs(compensated ? visualDelta : residual) > bound };
}
