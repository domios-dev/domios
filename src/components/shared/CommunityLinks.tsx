import { siteLinks } from "../../../shared/siteLinks";
import { nativeBridge } from "../../lib/nativeBridge";
import { AboutLink, SettingsCard } from "../Settings/shared";
import type { DropdownMenuGroup } from "./DropdownMenu";

const community = [
  {name:"Telegram",url:siteLinks.telegram,icon:new URL("../../../web/public/community/telegram.svg",import.meta.url).href},
  {name:"Discord",url:siteLinks.discord,icon:new URL("../../../web/public/community/discord-black.svg",import.meta.url).href},
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
export function CommunityLinks() {
  return <SettingsCard title="Community">
    <div className="flex items-center gap-3">{community.map(link =>
      <AboutLink key={link.name} href={link.url} label={link.name} icon={<CommunityIcon source={link.icon} />}
        onOpen={url => {void nativeBridge.openUrl(url);}} />
    )}</div>
  </SettingsCard>;
}
