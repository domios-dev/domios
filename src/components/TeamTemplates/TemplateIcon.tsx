import { Users } from "lucide-react";

type TemplateIconData = import("../../../gen/TemplateIcon").TemplateIcon;

const IMAGE_DATA_URL = /^data:image\/(svg\+xml|png);base64,/;

/** The package icon is always an image element from its data URL, never inline SVG DOM. */
export function TemplateIcon({ icon, size = 32 }: { icon?: TemplateIconData | null; size?: number }) {
  const style = { width: size, height: size };
  if (icon && IMAGE_DATA_URL.test(icon.data_url)) {
    return <img src={icon.data_url} alt="" aria-hidden="true" data-template-icon="image" style={style}
      className="shrink-0 rounded-lg object-cover" draggable={false} />;
  }
  return (
    <span aria-hidden="true" data-template-icon="fallback" style={style}
      className="grid shrink-0 place-items-center rounded-lg bg-[color-mix(in_srgb,var(--ct-accent)_14%,transparent)] text-[var(--ct-accent)]">
      <Users size={Math.round(size * 0.45)} />
    </span>
  );
}
