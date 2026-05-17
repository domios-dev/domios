/**
 * Generic settings panel renderer driven by `SettingField[]` from a plugin.
 *
 * One sub-component per `SettingField` variant. New variants are added by
 * extending the `SettingField` union in `lib/types.ts` AND adding a case here.
 * No per-plugin React components, no `if (providerId === "claude")` branches.
 *
 * Reads/writes plugin settings under `agents.<providerId>.<key>` in the
 * top-level `AppConfig`. Caller passes the providerId; this component doesn't
 * know which plugin it's rendering for.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  AppConfig,
  BadgeStyle,
  BadgeVariant,
  ProviderConfig,
  SettingField,
} from "../../lib/types";

interface SchemaRendererProps {
  schema: SettingField[];
  providerId: string;
  config: AppConfig;
  onChange: (providerId: string, patch: Partial<ProviderConfig>) => void;
  hideActions?: boolean;
  hidePathLinks?: boolean;
}

export function SchemaRenderer({
  schema,
  providerId,
  config,
  onChange,
  hideActions,
  hidePathLinks,
}: SchemaRendererProps) {
  const providerConfig = config.agents?.[providerId] ?? {};
  return (
    <div className="space-y-5">
      {schema.map((field, i) => (
        <FieldRenderer
          key={i}
          field={field}
          providerId={providerId}
          providerConfig={providerConfig}
          config={config}
          onChange={onChange}
          hideActions={hideActions}
          hidePathLinks={hidePathLinks}
        />
      ))}
    </div>
  );
}

// Per-provider toggle inherits from [agent] when unset — mirrors backend
// `provider_config()` fallback so UI matches what runtime actually uses.
function globalToggleFallback(key: string, config: AppConfig): boolean | undefined {
  if (key === "resume_sessions") return config.agent?.resume_sessions;
  if (key === "resume_skip_permissions") return config.agent?.resume_skip_permissions;
  return undefined;
}

interface FieldRendererProps {
  field: SettingField;
  providerId: string;
  providerConfig: ProviderConfig;
  config: AppConfig;
  onChange: (providerId: string, patch: Partial<ProviderConfig>) => void;
  hideActions?: boolean;
  hidePathLinks?: boolean;
}

function FieldRenderer({
  field,
  providerId,
  providerConfig,
  config,
  onChange,
  hideActions,
  hidePathLinks,
}: FieldRendererProps) {
  switch (field.kind) {
    case "section":
      return (
        <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
          <div className="text-sm font-medium text-[var(--ct-fg)]">{field.title}</div>
          {field.description && (
            <div className="text-xs text-[var(--ct-muted-fg)]">{field.description}</div>
          )}
          {field.fields.map((sub, i) => (
            <FieldRenderer
              key={i}
              field={sub}
              providerId={providerId}
              providerConfig={providerConfig}
              config={config}
              onChange={onChange}
              hideActions={hideActions}
              hidePathLinks={hidePathLinks}
            />
          ))}
        </div>
      );

    case "toggle": {
      const value = (providerConfig[field.key] as boolean | undefined)
        ?? globalToggleFallback(field.key, config)
        ?? field.default;
      return (
        <ToggleField
          label={field.label}
          description={field.description}
          checked={value}
          onChange={(v) => onChange(providerId, { [field.key]: v })}
        />
      );
    }

    case "text": {
      const value = (providerConfig[field.key] as string | undefined) ?? field.default ?? "";
      return (
        <div className="space-y-1">
          <div className="text-xs text-[var(--ct-fg)]">{field.label}</div>
          <input
            type="text"
            value={value}
            placeholder={field.placeholder}
            onChange={(e) => onChange(providerId, { [field.key]: e.target.value })}
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
          />
          {field.description && (
            <div className="text-[10px] text-[var(--ct-muted)]">{field.description}</div>
          )}
        </div>
      );
    }

    case "number": {
      const value = (providerConfig[field.key] as number | undefined) ?? field.default ?? 0;
      return (
        <div className="space-y-1">
          <div className="text-xs text-[var(--ct-fg)]">{field.label}</div>
          <input
            type="number"
            value={value}
            min={field.min}
            max={field.max}
            onChange={(e) => onChange(providerId, { [field.key]: Number(e.target.value) })}
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
          />
        </div>
      );
    }

    case "select": {
      const value = (providerConfig[field.key] as string | undefined) ?? field.default ?? "";
      return (
        <div className="flex items-center justify-between">
          <span className="text-xs text-[var(--ct-fg)]">{field.label}</span>
          <select
            value={value}
            onChange={(e) => onChange(providerId, { [field.key]: e.target.value })}
            className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
          >
            {field.options.map(([id, label]) => (
              <option key={id} value={id}>{label}</option>
            ))}
          </select>
        </div>
      );
    }

    case "api_key":
      return (
        <div className="space-y-1.5">
          <div className="text-xs text-[var(--ct-fg)]">{field.label}</div>
          <div className="flex items-center gap-2 text-[10px] text-[var(--ct-muted-fg)]">
            <span>set via</span>
            <code className="px-1.5 py-0.5 rounded bg-[var(--ct-bg)] border border-[var(--ct-border)] font-mono">
              {field.env_var}
            </code>
            {field.link && (
              <a
                href={field.link}
                target="_blank"
                rel="noreferrer"
                className="text-[var(--ct-accent)] hover:underline"
              >
                docs
              </a>
            )}
          </div>
        </div>
      );

    case "info":
      return <InfoField title={field.title} body={field.body} level={field.level} />;

    case "path_link":
      if (hidePathLinks) return null;
      return <PathLinkField label={field.label} path={field.path} />;

    case "badge":
      return <BadgeField queryCommand={field.query_command} variants={field.variants} />;

    case "action":
      if (hideActions) return null;
      return (
        <ActionField
          id={field.id}
          label={field.label}
          command={field.command}
          stateQuery={field.state_query}
          confirm={field.confirm}
        />
      );

    case "when": {
      const dependent = providerConfig[field.field];
      const visible = JSON.stringify(dependent) === JSON.stringify(field.equals);
      if (!visible) return null;
      return (
        <>
          {field.then.map((sub, i) => (
            <FieldRenderer
              key={i}
              field={sub}
              providerId={providerId}
              providerConfig={providerConfig}
              config={config}
              onChange={onChange}
              hideActions={hideActions}
              hidePathLinks={hidePathLinks}
            />
          ))}
        </>
      );
    }

    default:
      // Exhaustiveness — unreachable.
      return null;
  }
}

// ── Sub-components ───────────────────────────────────────────────

function ToggleField({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="flex-1">
        <div className="text-xs text-[var(--ct-fg)]">{label}</div>
        {description && (
          <div className="text-[10px] text-[var(--ct-muted-fg)] mt-0.5">{description}</div>
        )}
      </div>
      <button
        type="button"
        onClick={() => onChange(!checked)}
        className={`relative w-9 h-5 rounded-full transition-colors flex-shrink-0 ${
          checked ? "bg-[var(--ct-accent)]" : "bg-[var(--ct-border)]"
        }`}
      >
        <span
          className={`absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform ${
            checked ? "translate-x-4" : ""
          }`}
        />
      </button>
    </div>
  );
}

function InfoField({
  title,
  body,
  level,
}: {
  title?: string;
  body: string;
  level: "info" | "warn" | "error";
}) {
  const colors: Record<typeof level, string> = {
    info: "text-[var(--ct-muted-fg)]",
    warn: "text-[var(--ct-yellow)]",
    error: "text-[var(--ct-red)]",
  };
  return (
    <div className={`text-[11px] ${colors[level]} leading-relaxed`}>
      {title && <div className="font-medium mb-1">{title}</div>}
      <div className="whitespace-pre-wrap">{body}</div>
    </div>
  );
}

function PathLinkField({ label, path }: { label: string; path: string }) {
  const onClick = useCallback(() => {
    invoke("open_folder", { path }).catch(() => {});
  }, [path]);
  return (
    <div className="flex items-center justify-between text-[10px] text-[var(--ct-muted)]">
      <div className="flex items-center gap-2">
        <span>{label}</span>
        <code className="font-mono">{path}</code>
      </div>
      <button onClick={onClick} className="text-[var(--ct-accent)] hover:underline">
        Open folder
      </button>
    </div>
  );
}

const BADGE_COLORS: Record<BadgeStyle, string> = {
  green: "bg-[var(--ct-green)]/20 text-[var(--ct-green)]",
  orange: "bg-[var(--ct-yellow)]/20 text-[var(--ct-yellow)]",
  red: "bg-[var(--ct-red)]/20 text-[var(--ct-red)]",
  gray: "bg-[var(--ct-border)] text-[var(--ct-muted-fg)]",
};

/**
 * Map a backend status query response to a variant id string.
 *
 * Recognized shapes (in order):
 *   - plain string                    → use as-is
 *   - { variant: "..." }              → explicit variant id
 *   - { update_available: true }      → "update"
 *   - { installed: true }             → "installed"  (e.g. check_rule_installed)
 *   - { hooks_installed: true }       → "installed"  (e.g. check_claude_hooks)
 *   - anything else                   → "missing"
 *
 * Plugins that need a unique state machine should return `{ variant: "..." }`
 * — the legacy boolean shapes are convenience adapters for existing commands.
 */
function mapQueryResultToVariantId(result: unknown): string {
  if (typeof result === "string") return result;
  if (result && typeof result === "object") {
    const r = result as Record<string, unknown>;
    if (typeof r.variant === "string") return r.variant;
    if (r.update_available === true) return "update";
    if (r.installed === true) return "installed";
    if (r.hooks_installed === true) return "installed";
  }
  return "missing";
}

function BadgeField({
  queryCommand,
  variants,
}: {
  queryCommand: string;
  variants: BadgeVariant[];
}) {
  const [variant, setVariant] = useState<BadgeVariant | null>(null);

  useEffect(() => {
    let active = true;
    invoke(queryCommand)
      .then((result) => {
        if (!active) return;
        const id = mapQueryResultToVariantId(result);
        const match = variants.find((v) => v.id === id) ?? null;
        setVariant(match);
      })
      .catch(() => {
        if (active) setVariant(null);
      });
    return () => {
      active = false;
    };
  }, [queryCommand, variants]);

  if (!variant) return null;
  return (
    <span
      className={`text-[9px] px-1.5 py-0.5 rounded-full inline-block ${BADGE_COLORS[variant.style]}`}
    >
      {variant.label}
    </span>
  );
}

function ActionField({
  id: _id,
  label,
  command,
  stateQuery,
  confirm,
}: {
  id: string;
  label: string;
  command: string;
  stateQuery?: string;
  confirm?: string;
}) {
  const [state, setState] = useState<{ label: string; style: "primary" | "secondary" | "danger" } | null>(null);
  const [busy, setBusy] = useState(false);
  const [justDone, setJustDone] = useState(false);

  const refreshState = useCallback(() => {
    if (!stateQuery) return;
    invoke<unknown>(stateQuery)
      .then((result) => {
        // Map common state-query shapes into a button label/style.
        // Reuses the same variant-id mapping as Badge so the same backend
        // command can drive both a status badge and an action button.
        const id = mapQueryResultToVariantId(result);
        const r = result as Record<string, unknown> | undefined;
        if (r && typeof r === "object" && typeof r.label === "string") {
          // Explicit { label, style } from a plugin overrides the mapping.
          const explicitStyle = (r.style as "primary" | "secondary" | "danger" | undefined) ?? "primary";
          setState({ label: r.label, style: explicitStyle });
          return;
        }
        switch (id) {
          case "update":
            setState({ label: "Update Available", style: "primary" });
            break;
          case "installed":
            setState({ label: "Reinstall", style: "secondary" });
            break;
          default:
            setState({ label, style: "primary" });
            break;
        }
      })
      .catch(() => setState({ label, style: "primary" }));
  }, [stateQuery, label]);

  useEffect(() => {
    if (stateQuery) refreshState();
  }, [stateQuery, refreshState]);

  useEffect(() => {
    if (!justDone) return;
    const t = setTimeout(() => setJustDone(false), 2000);
    return () => clearTimeout(t);
  }, [justDone]);

  const click = useCallback(async () => {
    if (confirm && !window.confirm(confirm)) return;
    setBusy(true);
    try {
      await invoke(command);
      setJustDone(true);
      if (stateQuery) refreshState();
    } catch (e) {
      console.error(`[action ${command}]`, e);
    } finally {
      setBusy(false);
    }
  }, [command, confirm, stateQuery, refreshState]);

  const effective = state ?? { label, style: "primary" as const };
  const klass = justDone
    ? "bg-[var(--ct-green)] text-white"
    : effective.style === "primary"
      ? "bg-[var(--ct-accent)] text-white hover:brightness-110"
      : effective.style === "danger"
        ? "bg-[var(--ct-red)] text-white hover:brightness-110"
        : "border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]";

  return (
    <button
      onClick={click}
      disabled={busy || justDone}
      className={`w-full py-1.5 text-xs rounded font-medium transition-all duration-300 ${klass}`}
    >
      {justDone ? "\u2713 Done" : busy ? "..." : effective.label}
    </button>
  );
}
