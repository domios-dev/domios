import { ChevronDown, Circle, PanelLeft, ToggleRight } from "lucide-react";
import { appearancePreviewStyle, type AppearanceLook } from "../../lib/appearance";
import { isLightColor } from "../../lib/themeCss";
import { wallpaperRenderStyle } from "../../lib/chatWallpaper";
import { useWallpaperAsset } from "../../lib/useMachineAppearance";
import type { CSSProperties } from "react";

export function AppearancePreview({ look, compact = false, peer }: { look: AppearanceLook; compact?: boolean; peer?: string }) {
  const light = isLightColor(look.theme.background);
  const asset = useWallpaperAsset(peer, look.wallpaper?.asset);
  const style = { ...appearancePreviewStyle(look), ...wallpaperRenderStyle(look.theme, look.wallpaper ?? undefined, light, asset?.url) } as CSSProperties;
  return (
    <div
      aria-label="Appearance preview"
      role="img"
      className="ct-appearance-preview"
      data-compact={compact || undefined}
      data-ct-theme={light ? "light" : "dark"}
      style={style}
    >
      <aside>
        <small><PanelLeft size={12} /> Workspaces</small>
        <div className="ct-appearance-preview__team"><Circle size={15} /> Media</div>
        <div className="ct-appearance-preview__tab"><i /> Ritchie</div>
        <div className="ct-appearance-preview__tab"><i /> Morse</div>
      </aside>
      <div className="ct-appearance-preview__chat">
        <span className="ct-chat-wallpaper" aria-hidden="true" />
        <div className="ct-user-bubble">Make it calm.</div>
        <p>The new look is ready. <span>View changes</span></p>
        {!compact && <code>domios appearance current</code>}
        <div className="ct-appearance-preview__actions">
          <span className="ct-appearance-primary">Apply</span><span>Cancel</span>
          <ToggleRight size={19} aria-hidden="true" />
        </div>
        {!compact && <div className="ct-appearance-preview__menu">General <ChevronDown size={12} /></div>}
        <div className="ct-appearance-preview__composer">Message</div>
      </div>
    </div>
  );
}
