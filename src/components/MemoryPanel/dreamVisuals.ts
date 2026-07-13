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

// Build palettes from current theme CSS vars so cards remain theme-aligned.
function readVar(name: string, fallback: string): string {
  if (typeof document === "undefined") return fallback;
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return raw || fallback;
}

function withAlpha(hex: string, alpha: number): string {
  const m = hex.replace("#", "").trim();
  if (m.length !== 3 && m.length !== 6) return hex;
  const full = m.length === 3 ? m.split("").map((c) => c + c).join("") : m;
  const r = parseInt(full.slice(0, 2), 16);
  const g = parseInt(full.slice(2, 4), 16);
  const b = parseInt(full.slice(4, 6), 16);
  return `rgba(${r},${g},${b},${alpha})`;
}

function darken(hex: string, factor: number): string {
  const m = hex.replace("#", "");
  if (m.length !== 6) return hex;
  const r = Math.round(parseInt(m.slice(0, 2), 16) * factor);
  const g = Math.round(parseInt(m.slice(2, 4), 16) * factor);
  const b = Math.round(parseInt(m.slice(4, 6), 16) * factor);
  return `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

function isLightHex(hex: string): boolean {
  const m = hex.replace("#", "").trim();
  if (m.length !== 6) return false;
  const r = parseInt(m.slice(0, 2), 16);
  const g = parseInt(m.slice(2, 4), 16);
  const b = parseInt(m.slice(4, 6), 16);
  return (r + g + b) / 3 > 160;
}

function paletteFromAccent(accent: string, bg: string): DreamPalette {
  const light = isLightHex(bg);
  // Tint surface to accent while keeping contrast:
  // dark themes keep saturation, light themes use a softer tint.
  const tinted = blendHex(bg, accent, light ? 0.10 : 0.18);
  const deep = light ? blendHex(bg, accent, 0.18) : darken(tinted, 0.85);
  // On dark backgrounds, ink is the accent washed toward white so it pops. On light backgrounds, ink stays dark so the body is readable.
  const ink = light
    ? blendHex(accent, "#101216", 0.78)
    : blendHex(accent, "#ffffff", 0.65);
  return {
    accent,
    glow: withAlpha(accent, light ? 0.45 : 0.35),
    surface: light
      ? `linear-gradient(155deg, ${tinted} 0%, ${deep} 100%)`
      : `linear-gradient(155deg, ${deep} 0%, ${tinted} 100%)`,
    ink,
  };
}

function blendHex(a: string, b: string, t: number): string {
  const pa = a.replace("#", "");
  const pb = b.replace("#", "");
  if (pa.length !== 6 || pb.length !== 6) return a;
  const ra = parseInt(pa.slice(0, 2), 16);
  const ga = parseInt(pa.slice(2, 4), 16);
  const ba = parseInt(pa.slice(4, 6), 16);
  const rb = parseInt(pb.slice(0, 2), 16);
  const gb = parseInt(pb.slice(2, 4), 16);
  const bb = parseInt(pb.slice(4, 6), 16);
  const r = Math.round(ra + (rb - ra) * t);
  const g = Math.round(ga + (gb - ga) * t);
  const bl = Math.round(ba + (bb - ba) * t);
  return `#${[r, g, bl].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

// Use 8 deterministic palette slots from live theme vars.
// Cache this per render call to avoid repeated CSS var reads.
function dreamPalettes(): DreamPalette[] {
  const bg = readVar("--ct-bg", "#0d1117");
  const accents = [
    readVar("--ct-magenta", "#a78bfa"),
    readVar("--ct-blue", "#60a5fa"),
    readVar("--ct-green", "#34d399"),
    readVar("--ct-yellow", "#fbbf24"),
    readVar("--ct-bright-magenta", "#f472b6"),
    readVar("--ct-bright-yellow", "#fb923c"),
    readVar("--ct-cyan", "#22d3ee"),
    readVar("--ct-red", "#f87171"),
  ];
  return accents.map((c) => paletteFromAccent(c, bg));
}

export function paletteFor(seed: string): DreamPalette {
  const palettes = dreamPalettes();
  return palettes[hash(seed) % palettes.length];
}

export function moodLabel(opts: {
  area: string | null;
  tags?: string[] | null;
  body?: string | null;
}): string {
  const { area, tags, body } = opts;
  if (area && area.length > 0 && area !== "_dreams") return area;
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

export interface ParsedDream {
  insight: string;
  log: string | null;
  icon: string | null;
  mood: string | null;
}

const INSIGHT_RE = /<insight>([\s\S]*?)<\/insight>/i;
const LOG_RE = /<log>([\s\S]*?)<\/log>/i;
const ICON_RE = /<icon>\s*([a-z][a-z0-9-]*)\s*<\/icon>/i;
const MOOD_RE = /<mood>\s*([^<\n]+?)\s*<\/mood>/i;

// Parse dream body blocks (`<insight>`, `<log>`, `<icon>`, `<mood>`).
// If none are present, treat full body as insight.
export function parseDream(body: string): ParsedDream {
  if (!body) return { insight: "", log: null, icon: null, mood: null };
  const insightMatch = body.match(INSIGHT_RE);
  const logMatch = body.match(LOG_RE);
  const iconMatch = body.match(ICON_RE);
  const moodMatch = body.match(MOOD_RE);
  if (insightMatch || logMatch || iconMatch || moodMatch) {
    return {
      insight: insightMatch ? insightMatch[1].trim() : "",
      log: logMatch ? logMatch[1].trim() : null,
      icon: iconMatch ? iconMatch[1].trim().toLowerCase() : null,
      mood: moodMatch ? moodMatch[1].trim() : null,
    };
  }
  return { insight: body.trim(), log: null, icon: null, mood: null };
}

export function hasRenderableDreamInsight(body: string | null | undefined): boolean {
  if (!body) return false;
  return parseDream(body).insight.trim().length > 0;
}

// Map allowed icon slugs to Lucide components.
// Keep this list closed to prompt-discovered values.
const NAMED_ICONS: Record<string, LucideIcon> = {
  moon: Moon,
  sparkles: Sparkles,
  star: Star,
  sun: Sun,
  cloud: Cloud,
  atom: Atom,
  brain: Brain,
  zap: Zap,
  flame: Flame,
  leaf: Leaf,
  gem: Gem,
  telescope: Telescope,
  eye: Eye,
  feather: Feather,
  book: Book,
  compass: Compass,
  key: Key,
  globe: Globe,
  server: Server,
  terminal: Terminal,
  code: Code,
  lock: Lock,
  database: Database,
  wifi: Wifi,
  home: Home,
  bot: Bot,
  cpu: Cpu,
  message: MessageSquare,
  music: Music,
  map: Map,
  hammer: Hammer,
  headphones: Headphones,
  wand: Wand2,
  bug: Bug,
  branch: GitBranch,
  ship: Ship,
  mountain: Mountain,
  waves: Waves,
  wind: Wind,
  anchor: Anchor,
  heart: Heart,
  coffee: Coffee,
  activity: Activity,
};

export const ALLOWED_ICON_SLUGS = Object.keys(NAMED_ICONS);

export function iconBySlug(slug: string | null | undefined): LucideIcon | null {
  if (!slug) return null;
  return NAMED_ICONS[slug.toLowerCase()] ?? null;
}
