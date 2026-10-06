import type {
  ProviderPlugin,
  ProviderEvent,
  RendererProps,
} from "../../../src/lib/plugins/types";

const titlePattern = /aider/i;
const noisePatterns: RegExp[] = [/^[\s─━═█▀▄▌▐]+$/];

function Renderer({ raw }: RendererProps) {
  return raw();
}

const plugin: ProviderPlugin = {
  meta: {
    id: "aider",
    displayName: "Aider",
  },
  detect: (sample) => {
    if (sample.title && titlePattern.test(sample.title)) return true;
    const bytes = String(sample.bytes || "");
    return bytes.includes("aider") || bytes.includes("Aider");
  },
  parseLine: (_line: string): ProviderEvent[] => [],
  statusOf: () => null,
  Renderer,
  actions: [],
  noisePatterns,
  titlePattern,
};

export default plugin;
