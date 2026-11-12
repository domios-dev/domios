import { Users } from "lucide-react";

type TemplateIconData = import("../../../gen/TemplateIcon").TemplateIcon;

const IMAGE_DATA_URL = /^data:image\/(svg\+xml|png);base64,/;
const TILE = "grid shrink-0 place-items-center overflow-hidden bg-[color-mix(in_srgb,var(--ct-accent)_14%,transparent)] text-[var(--ct-accent)]";

/**
 * The package icon is always an image element from its data URL, never inline SVG DOM. A PNG fills its tile; an SVG glyph
 * and the fallback sit in the same tinted tile, so every row's icon column lines up.
 */
export function TemplateIcon({ icon, size = 32 }: { icon?: TemplateIconData | null; size?: number }) {
  const radius = Math.round(size / 4);
  const style = { width: size, height: size, borderRadius: radius };
  if (icon && IMAGE_DATA_URL.test(icon.data_url)) {
    if (icon.media_type !== "image/svg+xml") {
      return <img src={icon.data_url} alt="" aria-hidden="true" data-template-icon="image" style={style}
        className="shrink-0 object-cover" draggable={false} />;
    }
    const glyph = Math.round(size * 0.62);
    return (
      <span aria-hidden="true" data-template-icon="glyph" style={style} className={TILE}>
        <img src={icon.data_url} alt="" style={{ width: glyph, height: glyph }} draggable={false} />
      </span>
    );
  }
  return (
    <span aria-hidden="true" data-template-icon="fallback" style={style} className={TILE}>
      <Users size={Math.round(size * 0.45)} />
    </span>
  );
}
