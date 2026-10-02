export type ProviderLogoId = 'claude' | 'codex' | 'opencode' | 'cursor' | 'grok' | 'lmstudio';
type ProviderTheme = 'light' | 'dark';

type ProviderLogoAsset = {
  light: string;
  dark: string;
  width: number;
  height: number;
};

const providerLogos: Record<ProviderLogoId, ProviderLogoAsset> = {
  claude: { light: '/providers/claude-spark.svg', dark: '/providers/claude-spark.svg', width: 94, height: 94 },
  codex: { light: '/providers/openai-blossom.png', dark: '/providers/openai-blossom.png', width: 48, height: 48 },
  opencode: { light: '/providers/opencode-light.svg', dark: '/providers/opencode-dark.svg', width: 234, height: 42 },
  cursor: { light: '/providers/cursor-light.svg', dark: '/providers/cursor-dark.svg', width: 467, height: 532 },
  grok: { light: '/providers/grok.svg', dark: '/providers/grok.svg', width: 512, height: 512 },
  lmstudio: { light: '/providers/lm-studio-light.svg', dark: '/providers/lm-studio-dark.svg', width: 1152, height: 256 },
};

export function ProviderLogo({ mark, theme }: { mark: ProviderLogoId; theme: ProviderTheme }) {
  const asset = providerLogos[mark];

  return <img
    className="provider-logo"
    src={asset[theme]}
    alt=""
    width={asset.width}
    height={asset.height}
    loading="eager"
    decoding="async"
  />;
}
