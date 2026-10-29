import type { CSSProperties } from "react";
import { createPortal } from "react-dom";
import { AgentProviderPicker } from "../../../../src/components/AgentPicker/AgentProviderPicker";
import { setAgentDescriptors } from "../../../../src/lib/agentDescriptorCache";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import type { ProviderDescriptor } from "../../../../src/lib/types";
import { defineSpecimen } from "../types";
import catalogue from "./first-run-providers.json";

const LIGHT_THEME = {
  ...DEFAULT_THEME,
  name: "github-light",
  background: "#ffffff",
  foreground: "#1f2328",
  cursor: "#0969da",
  selection: "#b6e3ff",
  black: "#24292f",
  blue: "#0969da",
  white: "#6e7781",
  bright_black: "#57606a",
  bright_blue: "#218bff",
  bright_white: "#8c959f",
};

const providers: ProviderDescriptor[] = catalogue.map(({ id, display_name, badge, icon_svg, icon_color }) => ({
  schema_version: 1,
  id,
  display_name,
  badge,
  icon_svg,
  icon_color,
  transport: "pty",
  availability: "available",
  system_prompt_delivery: { cli_flag: "--system" },
  available_models: [],
  settings_schema: [],
  notifications_schema: [],
  supports_lazy_skills: false,
  has_hooks: false,
}) as ProviderDescriptor);

// The cards draw their glyphs from the live catalogue; the gallery has no daemon, so seed it.
setAgentDescriptors(providers);

type Props = { theme: "dark" | "light"; loading?: boolean; busy?: boolean; solo: string };

function WelcomeFrame({ theme, loading = false, busy = false, solo }: Props) {
  // `?solo=<state>` pins one state over the gallery so a viewport capture shows exactly the product stage.
  const pinned = new URLSearchParams(window.location.search).get("solo") === solo;
  const vars = Object.fromEntries(Object.entries(themeToCSS(theme === "light" ? LIGHT_THEME : DEFAULT_THEME))) as CSSProperties;
  const frame: CSSProperties = pinned ? { position: "fixed", inset: 0, zIndex: 9999 } : { width: "100%", height: 640 };
  const stage = <div data-ct-theme={theme} style={{ ...vars, ...frame, colorScheme: theme, background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
    <AgentProviderPicker
      headline="Welcome to Domios"
      subhead="Pick the agent that runs your General tab. You can change it later in Settings."
      providers={providers}
      providersLoading={loading}
      confirmLabel="Start General agent"
      busy={busy}
      onConfirm={() => {}}
      belowConfirm={<p className="m-0 text-[11px] text-[var(--ct-muted-fg)]">Product analytics is on. Turn off in <a href="#settings">Settings</a>.</p>}
    />
  </div>;
  return pinned ? createPortal(stage, document.body) : stage;
}

export default defineSpecimen({
  id: "first-run-picker",
  title: "Welcome · General agent picker",
  category: "Onboarding",
  where: "src/components/AgentPicker/AgentProviderPicker.tsx · ProviderCard, BrandMark, DotMatrixCanvas",
  stage: "full",
  notes: "The first screen of Domios. The boot cover's dot field carries on behind the picker: the mark opens once and releases a wave, the chosen card answers with its own wave, and the mark beats while the General agent starts. Reduced motion keeps the resting field and a still mark.",
  Component: WelcomeFrame,
  states: [
    { id: "dark", label: "Dark · ready", props: { theme: "dark", solo: "dark" } },
    { id: "light", label: "Light · ready", props: { theme: "light", solo: "light" } },
    { id: "loading", label: "Providers loading", props: { theme: "dark", loading: true, solo: "loading" } },
    { id: "busy", label: "Starting the General agent", props: { theme: "dark", busy: true, solo: "busy" } },
  ],
});
