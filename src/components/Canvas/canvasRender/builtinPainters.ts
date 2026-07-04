
import { buildCardContent, pickLevel } from "./cardContent";
import { drawCard } from "./drawCard";
import { isCanvasNodeExpandable, registerCanvasPainter, type CanvasPainter } from "./painterRegistry";

function cardPainter(kind: string): CanvasPainter {
  return {
    signature: (model) => buildCardContent(kind, model)?.hash ?? null,
    paint: (ctx, { model, w, h, palette }) => {
      const content = buildCardContent(kind, model);
      if (!content) return [];
      return drawCard(ctx, content, w, h, pickLevel(content, w, h), palette, isCanvasNodeExpandable(kind));
    },
  };
}

let registered = false;

// Idempotent — safe to call from every CanvasWorldCanvas mount.
export function registerBuiltinPainters(): void {
  if (registered) return;
  registered = true;
  registerCanvasPainter("plan", cardPainter("plan"));
  registerCanvasPainter("spec", cardPainter("spec"));
  registerCanvasPainter("task", cardPainter("task"));
  // Migrated from the retired DOM cards so these kinds stay visible on the canvas.
  registerCanvasPainter("handoff", cardPainter("handoff"));
  registerCanvasPainter("plan-ghost", cardPainter("plan-ghost"));
}
