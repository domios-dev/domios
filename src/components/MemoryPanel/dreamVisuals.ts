import {
  Moon,
  Sparkles,
  Star,
  Sun,
  Cloud,
  Atom,
  Brain,
  Zap,
  Flame,
  Leaf,
  Gem,
  Telescope,
  Eye,
  Feather,
  Book,
  Compass,
  Key,
  Globe,
  Server,
  Terminal,
  Code,
  Lock,
  Database,
  Wifi,
  Home,
  Bot,
  Cpu,
  MessageSquare,
  Music,
  Map,
  Hammer,
  Headphones,
  Wand2,
  Bug,
  GitBranch,
  Ship,
  Mountain,
  Waves,
  Wind,
  Anchor,
  Heart,
  Coffee,
  Activity,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

const AREA_ICONS: Record<string, LucideIcon> = {
  aurora: Sparkles,
  audio: Music,
  music: Music,
  sound: Headphones,
  ios: Code,
  swift: Code,
  android: Code,
  react: Code,
  frontend: Code,
  ui: Code,
  backend: Server,
  api: Server,
  infra: Server,
  deploy: Ship,
  build: Hammer,
  ci: Hammer,
  release: Ship,
  auth: Lock,
  security: Lock,
  crypto: Key,
  db: Database,
  database: Database,
  storage: Database,
  search: Eye,
  vector: Atom,
  embedding: Atom,
  ai: Brain,
  llm: Brain,
  agent: Bot,
  worker: Cpu,
  prompt: MessageSquare,
  chat: MessageSquare,
  mesh: Wifi,
  network: Wifi,
  remote: Globe,
  home: Home,
  automation: Wand2,
  schedule: Compass,
  cron: Compass,
  mem: Book,
  memory: Book,
  graph: GitBranch,
  bug: Bug,
  fix: Bug,
  test: Activity,
  perf: Zap,
  energy: Zap,
  health: Heart,
  social: Coffee,
  trip: Map,
  travel: Map,
  ocean: Waves,
  mountain: Mountain,
  wind: Wind,
  ship: Anchor,
  terminal: Terminal,
  cli: Terminal,
  flame: Flame,
  green: Leaf,
};

const KEYWORD_ICONS: Array<[RegExp, LucideIcon]> = [
  [/\bbug|fix|crash|error|broken\b/i, Bug],
  [/\bdeploy|release|ship|publish\b/i, Ship],
  [/\bauth|login|token|jwt|password\b/i, Lock],
  [/\bdatabase|sql|sqlite|postgres|mysql\b/i, Database],
  [/\bnetwork|mesh|tailscale|tcp|udp\b/i, Wifi],
  [/\baudio|sound|music|spectro\b/i, Music],
  [/\btest|spec|regression|eval\b/i, Activity],
  [/\bgraph|cluster|community|node\b/i, GitBranch],
  [/\bagent|worker|spawn|orchestr\b/i, Bot],
  [/\bprompt|llm|gpt|claude\b/i, MessageSquare],
  [/\bsearch|query|find\b/i, Eye],
  [/\bperf|speed|latency|fast\b/i, Zap],
  [/\btrip|travel|china|riga|latvia\b/i, Map],
  [/\bhome|automation|hass\b/i, Home],
];

const FALLBACKS: LucideIcon[] = [Moon, Sparkles, Star, Cloud, Telescope, Feather, Gem, Sun];

export function iconForDream(opts: {
  area: string | null;
  tags?: string[] | null;
  body?: string | null;
  title?: string | null;
}): LucideIcon {
  const { area, tags, body, title } = opts;
  if (area) {
    const slug = area.toLowerCase().split(/[^a-z]+/).filter(Boolean);
    for (const part of slug) {
      if (AREA_ICONS[part]) return AREA_ICONS[part];
    }
  }
  if (tags) {
    for (const t of tags) {
      const slug = t.toLowerCase().split(/[^a-z]+/).filter(Boolean);
      for (const part of slug) {
        if (AREA_ICONS[part]) return AREA_ICONS[part];
      }
    }
  }
  const haystack = `${title ?? ""} ${body ?? ""}`;
  for (const [re, ic] of KEYWORD_ICONS) {
    if (re.test(haystack)) return ic;
  }
  // Stable fallback by hash so the same entry always gets the same dream icon.
  const h = hash(`${title}:${area ?? ""}`);
  return FALLBACKS[h % FALLBACKS.length];
}

function hash(s: string): number {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export interface DreamPalette {
  accent: string;
  glow: string;
  surface: string;
  ink: string;
}

const DREAM_PALETTES: DreamPalette[] = [
  { accent: "#a78bfa", glow: "rgba(167,139,250,0.35)", surface: "linear-gradient(155deg,#1a1530 0%,#2a1f5c 100%)", ink: "#ddd6fe" },
  { accent: "#60a5fa", glow: "rgba(96,165,250,0.35)", surface: "linear-gradient(155deg,#0e1a2e 0%,#162d52 100%)", ink: "#dbeafe" },
  { accent: "#34d399", glow: "rgba(52,211,153,0.30)", surface: "linear-gradient(155deg,#0f1f1a 0%,#143f30 100%)", ink: "#d1fae5" },
  { accent: "#fbbf24", glow: "rgba(251,191,36,0.32)", surface: "linear-gradient(155deg,#1d1808 0%,#3a2a08 100%)", ink: "#fef3c7" },
  { accent: "#f472b6", glow: "rgba(244,114,182,0.32)", surface: "linear-gradient(155deg,#21111d 0%,#4a1a3a 100%)", ink: "#fce7f3" },
  { accent: "#fb923c", glow: "rgba(251,146,60,0.32)", surface: "linear-gradient(155deg,#241208 0%,#4d2410 100%)", ink: "#ffedd5" },
  { accent: "#22d3ee", glow: "rgba(34,211,238,0.32)", surface: "linear-gradient(155deg,#0a1f24 0%,#103a48 100%)", ink: "#cffafe" },
  { accent: "#f87171", glow: "rgba(248,113,113,0.30)", surface: "linear-gradient(155deg,#21111c 0%,#4a1818 100%)", ink: "#fee2e2" },
];

export function paletteFor(seed: string): DreamPalette {
  return DREAM_PALETTES[hash(seed) % DREAM_PALETTES.length];
}

export function moodLabel(opts: {
  area: string | null;
  tags?: string[] | null;
  body?: string | null;
}): string {
  const { area, tags, body } = opts;
  if (area && area.length > 0) return area;
  if (tags && tags.length > 0) {
    const interesting = tags.find((t) => t !== "dream-diary");
    if (interesting) return interesting;
  }
  if (body) {
    const m = body.match(/\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)?)\b/);
    if (m) return m[1].toLowerCase();
  }
  return "drift";
}
