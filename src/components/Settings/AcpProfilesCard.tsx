import { useState } from "react";
import { Boxes } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import { LaunchProfilePicker } from "../Terminal/LaunchProfilePicker";
import { useLaunchProfiles } from "../Terminal/useLaunchProfiles";
import { SettingsCard } from "./shared";

/**
 * The ACP agents this harness can start, with the same rows, install confirmation, progress and General agent
 * hand-off as Start a session. Registry search and removal arrive here once the daemon exposes them over HTTP;
 * until then the General agent does both through `domios agent acp`.
 */
export function AcpProfilesCard({ provider }: { provider: ProviderDescriptor }) {
  const catalog = useLaunchProfiles(provider);
  const [selected, setSelected] = useState("");
  return (
    <SettingsCard icon={<Boxes size={16} />} title="Agents" description="The ACP agents Domios can start. Pick one to install or set it up.">
      <div data-testid="settings-acp-profiles">
        <LaunchProfilePicker provider={provider} profiles={catalog.profiles} selected={selected}
          onSelect={setSelected} onRefresh={catalog.refresh} error={catalog.error} />
      </div>
    </SettingsCard>
  );
}
