import type { MachineTone } from "../../components/MeshPanel/accountCanvas";
import type { CloudCostSummary, CloudLifecycleState, CloudRate, CloudReceipt, CloudUsagePayload } from "./types";

export function formatCreditHours(hours: number): string {
  if (!Number.isFinite(hours) || hours <= 0) return "0 h";
  const rounded = Math.round(hours * 10) / 10;
  return `${Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1)} h`;
}

export function formatCurrencyAmount(amountMajor: number, currency = "USD"): string {
  const code = currency.trim().toUpperCase() || "USD";
  const whole = Number.isInteger(amountMajor);
  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: code,
      minimumFractionDigits: whole ? 0 : 2,
      maximumFractionDigits: whole ? 0 : 2,
    }).format(amountMajor);
  } catch {
    return `${code} ${whole ? String(amountMajor) : amountMajor.toFixed(2)}`;
  }
}

export function formatHourlyRate(amountMajor: number, currency = "USD"): string {
  return `${formatCurrencyAmount(amountMajor, currency)}/h`;
}

export function formatHourlyRateCents(cents: number, currency = "USD"): string {
  return formatHourlyRate(cents / 100, currency);
}

export function formatHourlyRateWords(amountMajor: number, currency = "USD"): string {
  return `${formatCurrencyAmount(amountMajor, currency)} an hour`;
}

export function formatCloudUptime(uptimeSecs: number): string {
  if (!Number.isFinite(uptimeSecs) || uptimeSecs <= 0) return "0m";
  const totalMinutes = Math.floor(uptimeSecs / 60);
  if (totalMinutes < 1) return "<1m";
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours === 0) return `${minutes}m`;
  if (minutes === 0) return `${hours}h`;
  return `${hours}h ${minutes}m`;
}

export function sizeTitle(size: string): string {
  const clean = size.trim();
  if (!clean) return "Small";
  return clean.charAt(0).toUpperCase() + clean.slice(1).toLowerCase();
}

export interface CloudSizeOption {
  id: string;
  label: string;
  creditsPerHour: number;
  hourlyRateCents: number | null;
  currency: string;
  rateLabel: string | null;
  specsLabel: string | null;
}

export function cloudSizeOptions(
  usage: CloudUsagePayload | null | undefined,
  fallbackRate?: CloudRate | null,
): CloudSizeOption[] {
  const currency = (usage?.currency ?? fallbackRate?.currency ?? "USD").toUpperCase();
  const firstPack = usage?.packs?.[0];
  const unitCents =
    firstPack && firstPack.hours > 0
      ? Math.round(firstPack.price_cents / firstPack.hours)
      : fallbackRate && fallbackRate.amount > 0 && (fallbackRate.credits_per_hour ?? 1) > 0
        ? Math.round((fallbackRate.amount * 100) / (fallbackRate.credits_per_hour ?? 1))
        : null;

  if (usage?.sizes && usage.sizes.length > 0) {
    return usage.sizes.map((entry) => {
      const specs = entry.cpu && entry.ram_gb ? `${entry.cpu} CPU, ${entry.ram_gb} GB` : null;
      return {
        id: entry.id,
        label: sizeTitle(entry.id),
        creditsPerHour: entry.credits_per_hour,
        hourlyRateCents: entry.hourly_rate_cents,
        currency,
        rateLabel: formatHourlyRateCents(entry.hourly_rate_cents, currency),
        specsLabel: specs,
      };
    });
  }

  const burnEntries = usage?.burn_per_size ? Object.entries(usage.burn_per_size) : [];
  const entries = burnEntries.length > 0 ? burnEntries : [["small", 1] as [string, number]];
  entries.sort((a, b) => a[1] - b[1]);
  return entries.map(([size, burn]) => {
    const hourlyRateCents = unitCents !== null ? Math.round(unitCents * burn) : null;
    return {
      id: size,
      label: sizeTitle(size),
      creditsPerHour: burn,
      hourlyRateCents,
      currency,
      rateLabel: hourlyRateCents !== null ? formatHourlyRateCents(hourlyRateCents, currency) : null,
      specsLabel: null,
    };
  });
}

export function sizeMultiplierHint(
  selected: CloudSizeOption,
  sizes: readonly CloudSizeOption[],
  isFree: boolean,
): string | null {
  if (isFree) {
    return selected.rateLabel ? `${selected.rateLabel} · sleeps when idle` : "Sleeps when idle";
  }
  const small = sizes.find((item) => item.id === "small") ?? sizes[0];
  const larger = sizes.find((item) => item.creditsPerHour > (small?.creditsPerHour ?? 1));
  if (larger && small && small.creditsPerHour > 0) {
    const ratio = larger.creditsPerHour / small.creditsPerHour;
    if (ratio === 2) return `${larger.label} uses hours twice as fast`;
    return `${larger.label} uses hours ${ratio}× as fast`;
  }
  return selected.rateLabel ? `${selected.rateLabel} while running` : null;
}

export type CloudEdgeKind = "solid" | "dashed" | "preparing";
export type CloudListIcon = "dot" | "moon" | "dots";

export interface CloudBlockedCardView {
  title: string;
  body: string;
  leftLabel: string;
  ctaLabel: string;
}

export interface CloudMachineView {
  state: CloudLifecycleState;
  sizeMeta: string;
  sizeLabel: string;
  rateLabel: string | null;
  footerLabel: string;
  footerSecondary: string | null;
  footerTone: MachineTone;
  connecting: boolean;
  statusIcon: "moon" | null;
  showVersion: boolean;
  dimCard: boolean;
  tileMuted: boolean;
  edgeKind: CloudEdgeKind;
  listStatus: string;
  listIcon: CloudListIcon;
  listOnline: boolean;
  blockedCard: CloudBlockedCardView | null;
  drawerStatusLabel: string;
  drawerStatusTone: MachineTone;
}

function machineRate(
  machine: Pick<CloudReceipt | CloudCostSummary, "size" | "rate_per_hour">,
  usage?: CloudUsagePayload | null,
): { amount: number; currency: string } | null {
  if (machine.rate_per_hour && Number.isFinite(machine.rate_per_hour.amount) && machine.rate_per_hour.amount > 0) {
    return { amount: machine.rate_per_hour.amount, currency: machine.rate_per_hour.currency };
  }
  const match = cloudSizeOptions(usage).find((option) => option.id === machine.size);
  if (match?.hourlyRateCents != null) {
    return { amount: match.hourlyRateCents / 100, currency: match.currency };
  }
  return null;
}

export function cloudMachineCardView(
  machine: Pick<CloudReceipt | CloudCostSummary, "size" | "state" | "rate_per_hour">,
  usage?: CloudUsagePayload | null,
): CloudMachineView {
  const sizeMeta = machine.size.toLowerCase();
  const sizeLabel = sizeTitle(machine.size);
  const rate = machineRate(machine, usage);
  const rateLabel = rate ? formatHourlyRate(rate.amount, rate.currency) : null;

  switch (machine.state) {
    case "creating":
    case "enrolling":
      return {
        state: machine.state,
        sizeMeta,
        sizeLabel,
        rateLabel,
        footerLabel: "Preparing your machine…",
        footerSecondary: null,
        footerTone: "progress",
        connecting: true,
        statusIcon: null,
        showVersion: false,
        dimCard: false,
        tileMuted: false,
        edgeKind: "preparing",
        listStatus: "preparing",
        listIcon: "dots",
        listOnline: false,
        blockedCard: null,
        drawerStatusLabel: "Preparing your machine…",
        drawerStatusTone: "progress",
      };
    case "awake":
    case "draining":
      return {
        state: machine.state,
        sizeMeta,
        sizeLabel,
        rateLabel,
        footerLabel: "Running",
        footerSecondary: sizeLabel,
        footerTone: "ok",
        connecting: false,
        statusIcon: null,
        showVersion: true,
        dimCard: false,
        tileMuted: false,
        edgeKind: "solid",
        listStatus: "online",
        listIcon: "dot",
        listOnline: true,
        blockedCard: null,
        drawerStatusLabel: "Running",
        drawerStatusTone: "ok",
      };
    case "waking":
      return {
        state: machine.state,
        sizeMeta,
        sizeLabel,
        rateLabel,
        footerLabel: "Waking up…",
        footerSecondary: sizeLabel,
        footerTone: "progress",
        connecting: true,
        statusIcon: null,
        showVersion: true,
        dimCard: false,
        tileMuted: false,
        edgeKind: "preparing",
        listStatus: "waking up",
        listIcon: "dots",
        listOnline: false,
        blockedCard: null,
        drawerStatusLabel: "Waking up…",
        drawerStatusTone: "progress",
      };
    case "out_of_credits": {
      const ratePart = rateLabel ? `${sizeLabel} runs at ${rateLabel} and wakes as soon as you top up.` : "Wakes as soon as you top up.";
      return {
        state: machine.state,
        sizeMeta,
        sizeLabel,
        rateLabel,
        footerLabel: "Asleep",
        footerSecondary: null,
        footerTone: "muted",
        connecting: false,
        statusIcon: "moon",
        showVersion: true,
        dimCard: false,
        tileMuted: true,
        edgeKind: "dashed",
        listStatus: "asleep, out of cloud hours",
        listIcon: "moon",
        listOnline: false,
        blockedCard: {
          title: "Out of cloud hours",
          body: `Your files and held message are waiting. ${ratePart}`,
          leftLabel: `${formatCreditHours(usage?.remaining ?? 0)} left`,
          ctaLabel: "Top up to wake",
        },
        drawerStatusLabel: "Out of cloud hours",
        drawerStatusTone: "muted",
      };
    }
    case "failed":
      return {
        state: machine.state,
        sizeMeta,
        sizeLabel,
        rateLabel,
        footerLabel: "Couldn't prepare the machine",
        footerSecondary: null,
        footerTone: "danger",
        connecting: false,
        statusIcon: null,
        showVersion: false,
        dimCard: false,
        tileMuted: true,
        edgeKind: "dashed",
        listStatus: "asleep",
        listIcon: "moon",
        listOnline: false,
        blockedCard: null,
        drawerStatusLabel: "Couldn't prepare the machine",
        drawerStatusTone: "danger",
      };
    case "asleep":
    case "deleted":
    default:
      return {
        state: machine.state,
        sizeMeta,
        sizeLabel,
        rateLabel,
        footerLabel: "Asleep",
        footerSecondary: "wakes when you send it work",
        footerTone: "muted",
        connecting: false,
        statusIcon: "moon",
        showVersion: true,
        dimCard: true,
        tileMuted: true,
        edgeKind: "dashed",
        listStatus: "asleep",
        listIcon: "moon",
        listOnline: false,
        blockedCard: null,
        drawerStatusLabel: "Asleep",
        drawerStatusTone: "muted",
      };
  }
}

export type CloudCreateDecision = "create" | "free_paywall" | "top_up";

export function cloudCreateDecision(input: {
  planId: string | null | undefined;
  remaining: number | null;
  fullAccess?: boolean;
}): CloudCreateDecision {
  if (!input.fullAccess && (!input.planId || input.planId === "free")) {
    return "free_paywall";
  }
  if (input.remaining !== null && input.remaining <= 0) {
    return "top_up";
  }
  return "create";
}

export type CloudFailureActionKind = "retry" | "upgrade" | "top_up";

export interface CloudFailurePresentation {
  message: string;
  action: CloudFailureActionKind;
  actionLabel: string;
}

export function cloudFailurePresentation(code: string | null | undefined): CloudFailurePresentation {
  switch (code) {
    case "credits_exhausted":
    case "lease_required":
      return {
        message: "Out of credits: top up to wake",
        action: "top_up",
        actionLabel: "Top up",
      };
    case "limit_reached":
    case "cloud.machines":
    case "plan_required":
      return {
        message: "Your plan's cloud machine limit has been reached.",
        action: "upgrade",
        actionLabel: "Upgrade plan",
      };
    case "rate_limited":
    case "rate-limited":
      return {
        message: "Cloud provisioning is busy. Try again in a moment.",
        action: "retry",
        actionLabel: "Try again",
      };
    default:
      return {
        message: "Couldn't prepare the machine",
        action: "retry",
        actionLabel: "Try again",
      };
  }
}
