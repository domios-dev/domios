import { siteLinks } from "../../../shared/siteLinks";
import { nativeBridge } from "../../lib/nativeBridge";
import { AboutLink, SettingsCard } from "../Settings/shared";
import type { DropdownMenuGroup } from "./DropdownMenu";
import { useId, type ReactNode } from "react";
import { Gift } from "lucide-react";
import { Button } from "./Button";

const community = [
  {name:"Telegram",url:siteLinks.telegram,icon:new URL("../../assets/community/telegram.svg",import.meta.url).href},
  {name:"Discord",url:siteLinks.discord,icon:new URL("../../assets/community/discord-black.svg",import.meta.url).href},
];
function CommunityIcon({source}:{source:string}) {
  if (source === community[0].icon) return <img src={source} alt="" width={16} height={16} className="shrink-0" />;
  return <span aria-hidden="true" className="inline-block h-4 w-4 shrink-0" style={{
    backgroundColor:"currentColor",maskImage:`url("${source}")`,maskSize:"contain",maskRepeat:"no-repeat",maskPosition:"center",
  }} />;
}
export function communityMenu(): DropdownMenuGroup {
  return {label:"Community",items:community.map(link => ({
    label:link.name,icon:<CommunityIcon source={link.icon} />,onClick:() => {void nativeBridge.openUrl(link.url);},
  }))};
}
/**
 * Stands in for every purchase call to action while in-app purchase is off: one calm sentence and
 * the two places that hand out free keys. `compact` drops the mark for tight spots such as a mesh card.
 */
export function CommunityKeysNotice({ compact = false }: { compact?: boolean }) {
  const titleId = useId();
  const links = [...community].sort((a, b) => a.name.localeCompare(b.name));
  return <section aria-labelledby={titleId} data-testid="community-keys-notice"
    className={`flex items-start gap-3 rounded-lg border border-[color-mix(in_srgb,var(--ct-accent)_22%,var(--ct-border))] bg-[color-mix(in_srgb,var(--ct-accent)_6%,var(--ct-bg))] ${compact ? "px-3 py-2.5" : "px-4 py-3.5"}`}>
    {!compact && <span aria-hidden="true" className="grid h-8 w-8 shrink-0 place-items-center rounded-full bg-[color-mix(in_srgb,var(--ct-accent)_14%,transparent)] text-[var(--ct-accent)]">
      <Gift size={16} strokeWidth={1.75} />
    </span>}
    <div className="flex min-w-0 flex-col gap-2">
      <div className="flex flex-col gap-0.5">
        <p id={titleId} className="m-0 text-[13px] font-medium text-[var(--ct-fg)]">Subscriptions are coming later.</p>
        <p className="m-0 text-xs leading-relaxed text-[var(--ct-muted-fg)]">For now, join our Discord or Telegram: we’re giving out free keys there.</p>
      </div>
      <div className="flex flex-wrap items-center gap-2">{links.map(link =>
        <Button key={link.name} type="button" variant="capsule" className="!pl-2.5 !pr-3 font-medium"
          onClick={() => {void nativeBridge.openUrl(link.url);}}>
          <CommunityIcon source={link.icon} />{link.name}
        </Button>
      )}</div>
    </div>
  </section>;
}
export function CommunityLinks({ icon }: { icon?: ReactNode }) {
  return <SettingsCard icon={icon} title="Community">
    <div className="flex items-center gap-3">{community.map(link =>
      <AboutLink key={link.name} href={link.url} label={link.name} icon={<CommunityIcon source={link.icon} />}
        onOpen={url => {void nativeBridge.openUrl(url);}} />
    )}</div>
  </SettingsCard>;
}
