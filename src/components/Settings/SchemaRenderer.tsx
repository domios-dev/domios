import { nativeBridge } from "../../lib/nativeBridge";
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
import { invoke } from "../../lib/bridge";
import { Button } from "../shared/Button";
import { Card } from "../shared/Card";
import { Chip, type ChipVariant } from "../shared/Chip";
import { FormRow } from "../shared/FormRow";
import { LiquidToggle } from "../shared/LiquidToggle";
import { mapQueryResultToVariantId, SchemaActionBar, segmentActionRuns } from "./SchemaActions";

import type {
  AppConfig,
  BadgeStyle,
  BadgeVariant,
  ProviderConfig,
  SettingField,
} from "../../lib/types";

type ArraySettingField = Extract<SettingField, { kind: "array" }>;
type ObjectArrayItemField = ArraySettingField["item"]["fields"][number];

interface SchemaRendererProps {
  schema: SettingField[];
  providerId: string;
  config: AppConfig;
  onChange: (providerId: string, patch: Partial<ProviderConfig>) => void;
  /**
   * When set, field values are read from this record instead of
   * `config.agents[providerId]`. Used by non-agent surfaces (plugin settings
   * read/write `config.plugins.<id>`). When provided, the agent-only
   * `[agent]` resume-toggle fallback is skipped.
   */
  values?: Record<string, unknown>;
  hideActions?: boolean;
  hidePathLinks?: boolean;
}

export function SchemaRenderer({
  schema,
  providerId,
  config,
  onChange,
  values,
  hideActions,
  hidePathLinks,
}: SchemaRendererProps) {
  const providerConfig = (values ?? config.agents?.[providerId] ?? {}) as ProviderConfig;
  return (
    <div className="space-y-5" data-testid="settings-schema" data-provider-id={providerId}>
      <FieldList
        fields={schema}
        providerId={providerId}
        providerConfig={providerConfig}
        config={config}
        onChange={onChange}
        agentNamespace={values === undefined}
        hideActions={hideActions}
        hidePathLinks={hidePathLinks}
      />
    </div>
  );
}

// fallback). Only meaningful for agent providers; plugin surfaces pass values.
function globalToggleFallback(key: string, config: AppConfig, agentNamespace: boolean): boolean | undefined {
  if (!agentNamespace) return undefined;
  if (key === "resume_sessions") return config.agent?.resume_sessions;
  return undefined;
}

interface FieldRendererProps {
  field: SettingField;
  providerId: string;
  providerConfig: ProviderConfig;
  config: AppConfig;
  onChange: (providerId: string, patch: Partial<ProviderConfig>) => void;
  agentNamespace: boolean;
  hideActions?: boolean;
  hidePathLinks?: boolean;
}

interface FieldListProps extends Omit<FieldRendererProps, "field"> {
  fields: SettingField[];
}

function FieldList({ fields, ...rest }: FieldListProps) {
  return (
    <>
      {segmentActionRuns(fields).map((segment, i) =>
        segment.kind === "actions"
          ? (rest.hideActions ? null : <SchemaActionBar key={i} actions={segment.actions} />)
          : <FieldRenderer key={i} field={segment.field} {...rest} />,
      )}
    </>
  );
}

function FieldRenderer({
  field,
  providerId,
  providerConfig,
  config,
  onChange,
  agentNamespace,
  hideActions,
  hidePathLinks,
}: FieldRendererProps) {
  switch (field.kind) {
    case "section": {
      const badges = field.fields.filter((sub) => sub.kind === "badge");
      const body = field.fields.filter((sub) => sub.kind !== "badge");
      return (
        <Card className="p-4 space-y-3" data-testid="settings-schema-section">
          <div className="space-y-1">
            <div className="flex items-center justify-between gap-3">
              <div className="text-sm font-medium text-[var(--ct-fg)]">{field.title}</div>
              {badges.length > 0 && (
                <div className="flex flex-none items-center gap-1.5">
                  <FieldList fields={badges}
                    providerId={providerId}
                    providerConfig={providerConfig}
                    config={config}
                    onChange={onChange}
                    agentNamespace={agentNamespace}
                    hideActions={hideActions}
                    hidePathLinks={hidePathLinks}
                  />
                </div>
              )}
            </div>
            {field.description && (
              <div className="text-xs leading-relaxed text-[var(--ct-muted-fg)]">{field.description}</div>
            )}
          </div>
          <FieldList
            fields={body}
            providerId={providerId}
            providerConfig={providerConfig}
            config={config}
            onChange={onChange}
            agentNamespace={agentNamespace}
            hideActions={hideActions}
            hidePathLinks={hidePathLinks}
          />
        </Card>
      );
    }

    case "toggle": {
      const value = (providerConfig[field.key] as boolean | undefined)
        ?? globalToggleFallback(field.key, config, agentNamespace)
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
        <div data-testid="settings-schema-field" data-settings-field={field.key}>
          <FormRow label={field.label} hint={field.description}>
            <input
              type="text"
              data-testid="settings-schema-text"
              data-settings-field={field.key}
              value={value}
              placeholder={field.placeholder}
              onChange={(e) => onChange(providerId, { [field.key]: e.target.value })}
              className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
            />
          </FormRow>
        </div>
      );
    }

    case "number": {
      const value = (providerConfig[field.key] as number | undefined) ?? field.default ?? 0;
      return (
        <div data-testid="settings-schema-field" data-settings-field={field.key}>
          <FormRow label={field.label}>
            <input
              type="number"
              data-testid="settings-schema-number"
              data-settings-field={field.key}
              value={value}
              min={field.min}
              max={field.max}
              onChange={(e) => onChange(providerId, { [field.key]: Number(e.target.value) })}
              className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
            />
          </FormRow>
        </div>
      );
    }

    case "select": {
      const value = (providerConfig[field.key] as string | undefined) ?? field.default ?? "";
      return (
        <div className="flex items-center justify-between" data-testid="settings-schema-field" data-settings-field={field.key}>
          <span className="text-xs text-[var(--ct-fg)]">{field.label}</span>
          <select
            data-testid="settings-schema-select"
            data-settings-field={field.key}
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

    case "array": {
      const stored = providerConfig[field.key];
      const value = Array.isArray(stored)
        ? (stored as Record<string, unknown>[])
        : field.default ?? [];
      return (
        <ObjectArrayField
          field={field}
          value={value}
          onChange={(next) => onChange(providerId, { [field.key]: next })}
        />
      );
    }

    case "api_key":
      return (
        <div className="space-y-1.5" data-testid="settings-schema-api-key" data-settings-field={field.env_var}>
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
      return (
        <BadgeField
          queryCommand={field.query_command}
          queryArgs={field.query_args}
          variants={field.variants}
        />
      );

    case "action":
      if (hideActions) return null;
      return <SchemaActionBar actions={[field]} />;

    case "when": {
      const dependent = providerConfig[field.field];
      const visible = JSON.stringify(dependent) === JSON.stringify(field.equals);
      if (!visible) return null;
      return (
        <>
          <FieldList
            fields={field.then}
            providerId={providerId}
            providerConfig={providerConfig}
            config={config}
            onChange={onChange}
            agentNamespace={agentNamespace}
            hideActions={hideActions}
            hidePathLinks={hidePathLinks}
          />
        </>
      );
    }

    default:
      // Exhaustiveness — unreachable.
      return null;
  }
}

// ── Sub-components ───────────────────────────────────────────────

function ObjectArrayField({
  field,
  value,
  onChange,
}: {
  field: ArraySettingField;
  value: Record<string, unknown>[];
  onChange: (next: Record<string, unknown>[]) => void;
}) {
  const itemLabel = field.item_label ?? field.label;
  const add = () => onChange([...value, createDefaultObject(field.item.fields)]);
  const remove = (index: number) => onChange(value.filter((_, i) => i !== index));
  const update = (index: number, key: string, nextValue: unknown) => {
    onChange(value.map((item, i) => (i === index ? { ...item, [key]: nextValue } : item)));
  };

  return (
    <div className="space-y-2" data-testid="settings-schema-array" data-settings-field={field.key}>
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-xs text-[var(--ct-fg)]">{field.label}</div>
          {field.description && (
            <div className="text-[10px] text-[var(--ct-muted)]">{field.description}</div>
          )}
        </div>
        <Button
          variant="ghost"
          size="compact"
          type="button"
          data-testid="settings-schema-array-add"
          data-settings-field={field.key}
          onClick={add}
          className="text-[10px]"
        >
          Add
        </Button>
      </div>
      <div className="space-y-3">
        {value.map((item, index) => (
          <Card
            key={index}
            className="p-3 space-y-2"
            data-testid="settings-schema-array-row"
          >
            <div className="flex items-center justify-between gap-2">
              <div className="text-[11px] font-medium text-[var(--ct-fg)]">
                {arrayRowTitle(item, itemLabel, index)}
              </div>
              <Button
                variant="danger"
                size="compact"
                type="button"
                data-testid="settings-schema-array-remove"
                data-settings-field={`${field.key}.${index}`}
                onClick={() => remove(index)}
                className="text-[10px]"
              >
                Remove
              </Button>
            </div>
            {field.item.fields.map((subField) => (
              <ObjectArrayItemInput
                key={subField.key}
                parentKey={field.key}
                index={index}
                field={subField}
                value={item[subField.key]}
                onChange={(next) => update(index, subField.key, next)}
              />
            ))}
          </Card>
        ))}
      </div>
    </div>
  );
}

function createDefaultObject(fields: ObjectArrayItemField[]): Record<string, unknown> {
  const next: Record<string, unknown> = {};
  for (const field of fields) {
    if (field.default !== undefined) {
      next[field.key] = field.default;
    } else if (field.kind === "number") {
      next[field.key] = 0;
    } else if (field.kind === "json") {
      next[field.key] = {};
    } else {
      next[field.key] = "";
    }
  }
  return next;
}

function arrayRowTitle(item: Record<string, unknown>, itemLabel: string, index: number): string {
  const name = typeof item.name === "string" ? item.name.trim() : "";
  const id = typeof item.id === "string" ? item.id.trim() : "";
  const suffix = name || id || `${itemLabel} ${index + 1}`;
  return suffix === itemLabel ? suffix : `${itemLabel}: ${suffix}`;
}

function ObjectArrayItemInput({
  parentKey,
  index,
  field,
  value,
  onChange,
}: {
  parentKey: string;
  index: number;
  field: ObjectArrayItemField;
  value: unknown;
  onChange: (next: unknown) => void;
}) {
  const dataField = `${parentKey}.${index}.${field.key}`;
  const label = <div className="text-[10px] text-[var(--ct-muted-fg)]">{field.label}</div>;

  if (field.kind === "number") {
    const numberValue = typeof value === "number" ? value : field.default ?? 0;
    return (
      <label className="block space-y-1">
        {label}
        <input
          type="number"
          data-testid="settings-schema-array-number"
          data-settings-field={dataField}
          value={numberValue}
          min={field.min}
          max={field.max}
          onInput={(e) => onChange(Number(e.currentTarget.value))}
          className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
        />
      </label>
    );
  }

  if (field.kind === "textarea") {
    const textValue = typeof value === "string" ? value : field.default ?? "";
    return (
      <label className="block space-y-1">
        {label}
        <textarea
          data-testid="settings-schema-array-textarea"
          data-settings-field={dataField}
          value={textValue}
          placeholder={field.placeholder}
          rows={4}
          onInput={(e) => onChange(e.currentTarget.value)}
          className="w-full resize-y bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
        />
      </label>
    );
  }

  if (field.kind === "json") {
    return (
      <JsonObjectInput
        dataField={dataField}
        field={field}
        value={isRecord(value) ? value : field.default ?? {}}
        onChange={onChange}
      />
    );
  }

  const textValue = typeof value === "string" ? value : field.default ?? "";
  return (
    <label className="block space-y-1">
      {label}
      <input
        type="text"
        data-testid="settings-schema-array-input"
        data-settings-field={dataField}
        value={textValue}
        placeholder={field.placeholder}
        onInput={(e) => onChange(e.currentTarget.value)}
        className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
      />
    </label>
  );
}

function JsonObjectInput({
  dataField,
  field,
  value,
  onChange,
}: {
  dataField: string;
  field: Extract<ObjectArrayItemField, { kind: "json" }>;
  value: Record<string, unknown>;
  onChange: (next: unknown) => void;
}) {
  const [draft, setDraft] = useState(() => JSON.stringify(value, null, 2));
  const [error, setError] = useState<string | null>(null);
  const valueKey = JSON.stringify(value);

  useEffect(() => {
    setDraft(JSON.stringify(value, null, 2));
    setError(null);
  }, [valueKey]);

  return (
    <label className="block space-y-1">
      <div className="text-[10px] text-[var(--ct-muted-fg)]">{field.label}</div>
      <textarea
        data-testid="settings-schema-array-json"
        data-settings-field={dataField}
        value={draft}
        rows={3}
        onInput={(e) => {
          const nextDraft = e.currentTarget.value;
          setDraft(nextDraft);
          try {
            const parsed = JSON.parse(nextDraft) as unknown;
            if (!isRecord(parsed)) {
              setError("JSON must be an object");
              return;
            }
            setError(null);
            onChange(parsed);
          } catch {
            setError("Invalid JSON");
          }
        }}
        className="w-full resize-y bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 font-mono text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
      />
      {error && <div className="text-[10px] text-[var(--ct-red)]">{error}</div>}
      {!error && field.description && <div className="text-[10px] text-[var(--ct-muted)]">{field.description}</div>}
    </label>
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

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
    <div className="flex items-start justify-between gap-3" data-testid="settings-schema-toggle-row">
      <div className="flex-1">
        <div className="text-xs text-[var(--ct-fg)]">{label}</div>
        {description && (
          <div className="text-[10px] text-[var(--ct-muted-fg)] mt-0.5">{description}</div>
        )}
      </div>
      <LiquidToggle
        data-testid="settings-schema-toggle"
        aria-label={label}
        checked={checked}
        onCheckedChange={onChange}
        className="flex-shrink-0"
      />
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
    <div className={`text-[11px] ${colors[level]} leading-relaxed`} data-testid="settings-schema-info" data-settings-level={level}>
      {title && <div className="font-medium mb-1">{title}</div>}
      <div className="whitespace-pre-wrap">{body}</div>
    </div>
  );
}

function PathLinkField({ label, path }: { label: string; path: string }) {
  const onClick = useCallback(() => {
    nativeBridge.openFolder(path).catch(() => {});
  }, [path]);
  return (
    <div className="flex min-w-0 items-center gap-3 text-[11px] text-[var(--ct-muted)]" data-testid="settings-schema-path-link">
      <span className="flex-none">{label}</span>
      <code className="ct-text-mono min-w-0 flex-1 truncate text-[var(--ct-muted-fg)]" title={path}>{path}</code>
      <Button variant="ghost" size="compact" onClick={onClick} className="flex-none" data-testid="settings-schema-open-folder">
        Open folder
      </Button>
    </div>
  );
}

function BadgeField({
  queryCommand,
  queryArgs,
  variants,
}: {
  queryCommand: string;
  queryArgs?: Record<string, unknown>;
  variants: BadgeVariant[];
}) {
  const [variant, setVariant] = useState<BadgeVariant | null>(null);
  // Serialize args for stable dependency tracking. Plain object refs flip every render even when contents don't change.
  const argsKey = JSON.stringify(queryArgs ?? null);
  const variantsKey = JSON.stringify(variants);

  useEffect(() => {
    let active = true;
    const args = argsKey === "null" ? undefined : (JSON.parse(argsKey) as Record<string, unknown>);
    invoke(queryCommand, args)
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
  }, [queryCommand, argsKey, variantsKey]);

  if (!variant) return null;
  return (
    <Chip
      variant={badgeVariant(variant.style)}
      data-testid="settings-schema-badge"
      className="text-[9px]"
    >
      {variant.label}
    </Chip>
  );
}

function badgeVariant(style: BadgeStyle): ChipVariant {
  if (style === "green") return "success";
  if (style === "red") return "danger";
  if (style === "orange") return "warning";
  return "muted";
}
