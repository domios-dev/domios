import { useState } from "react";
import { Info, KeyRound, Power, Send, Sparkles } from "lucide-react";
import { CopyRow, InfoRow, SettingsCard, SettingsPill, SettingsSection, SwitchTile, Toggle } from "../../../../src/components/Settings/shared";
import { defineSpecimen } from "../types";

type CardKind = "toggles" | "tiles" | "info" | "attention" | "header";

function StartupCard() {
  const [launch, setLaunch] = useState(true);
  const [librarian, setLibrarian] = useState(false);
  return <SettingsCard icon={<Power size={16} />} title="Startup" description="Resume the daemon, mesh and crons without opening the app." headerRight={<SettingsPill tone="green">on</SettingsPill>}>
    <Toggle label="Launch Domios on system start" checked={launch} onChange={setLaunch} />
    <Toggle label="Enable Librarian (background skill vectorizer)" checked={librarian} onChange={setLibrarian} />
  </SettingsCard>;
}

function TilesCard() {
  const [routes, setRoutes] = useState({ panel: true, sound: false, telegram: true });
  const set = (key: keyof typeof routes) => (value: boolean) => setRoutes((current) => ({ ...current, [key]: value }));
  return <SettingsCard icon={<Send size={16} />} title="Notifications" description="Where each level may interrupt you." headerRight={<SettingsPill tone="accent">2 routes</SettingsPill>}>
    <SettingsSection title="Needs attention">
      <SwitchTile label="Notification panel" hint="Always on the panel, even when muted elsewhere." checked={routes.panel} onChange={set("panel")} />
      <SwitchTile label="Sound" checked={routes.sound} onChange={set("sound")} dense />
      <SwitchTile label="Telegram" hint="Sent to the chat you wrote from last." checked={routes.telegram} onChange={set("telegram")} dense />
    </SettingsSection>
  </SettingsCard>;
}

function CardSpecimen({ kind }: { kind: CardKind }) {
  const card = {
    toggles: <StartupCard />,
    tiles: <TilesCard />,
    info: <SettingsCard icon={<Info size={16} />} title="Info">
      <InfoRow label="Version" value="1.14.0" />
      <InfoRow label="Daemon" value="127.0.0.1:7686" mono />
      <CopyRow label="Pairing code" value="ct-pair-7f2c-91ab" />
    </SettingsCard>,
    attention: <SettingsCard icon={<KeyRound size={16} />} title="Spinal Cord" description="A small model that answers first for memory, tab health and heartbeats." headerRight={<SettingsPill tone="amber">needs token</SettingsPill>}>
      <InfoRow label="Adapter" value="openrouter" />
      <InfoRow label="Model" value="haiku-5" mono />
    </SettingsCard>,
    header: <SettingsCard icon={<Sparkles size={16} />} title="Skills" description="Drop skill folders into ~/.codeterm/skills/; every provider sees them." headerRight={<SettingsPill tone="muted">auto-synced</SettingsPill>} />,
  }[kind];
  return <div className="gallery-fill">{card}</div>;
}

export default defineSpecimen({
  id: "settings-card",
  title: "Settings card",
  category: "Settings",
  where: "src/components/Settings/shared.tsx · SettingsCard, SettingsPill, Toggle, SwitchTile, SettingsSection",
  stage: "wide",
  notes: "The compact ct-card settings card: optional icon badge, the description under the title, a headerRight SettingsPill on the shared Chip tones and the body rows. Toggles and switch tiles are live: click them to watch the liquid travel.",
  Component: CardSpecimen,
  states: [
    { id: "toggles", label: "Icon · description · pill · toggles", props: { kind: "toggles" } },
    { id: "tiles", label: "Section label · switch tiles", props: { kind: "tiles" } },
    { id: "info", label: "Title only · info and copy rows", props: { kind: "info" } },
    { id: "attention", label: "Attention pill", props: { kind: "attention" } },
    { id: "header", label: "Header only · no body", props: { kind: "header" } },
  ],
});
