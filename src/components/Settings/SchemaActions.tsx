import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useEffect, useState } from "react";
import { Check } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { Button, type ButtonVariant } from "../shared/Button";
import type { ActionStyle, SettingField } from "../../lib/types";
import { RemoteLocalOnlyNotice } from "./shared";

export type ActionSetting = Extract<SettingField, { kind: "action" }>;
export type FieldSegment =
  | { kind: "field"; field: SettingField }
  | { kind: "actions"; actions: ActionSetting[] };

/**
 * Map a backend status query response to a variant id string.
 *
 * Recognized shapes (in order): plain string; `{ variant }`; `{ update_available: true }` → "update";
 * `{ installed | hooks_installed: true }` or `{ integration_state: "installed" }` → "installed";
 * anything else → "missing".
 */
export function mapQueryResultToVariantId(result: unknown): string {
  if (typeof result === "string") return result;
  if (result && typeof result === "object") {
    const r = result as Record<string, unknown>;
    if (typeof r.variant === "string") return r.variant;
    if (r.update_available === true) return "update";
    if (r.installed === true) return "installed";
    if (r.hooks_installed === true) return "installed";
    if (r.integration_state === "installed") return "installed";
  }
  return "missing";
}

export function segmentActionRuns(fields: readonly SettingField[]): FieldSegment[] {
  const segments: FieldSegment[] = [];
  for (const field of fields) {
    const last = segments[segments.length - 1];
    if (field.kind !== "action") segments.push({ kind: "field", field });
    else if (last?.kind === "actions") last.actions.push(field);
    else segments.push({ kind: "actions", actions: [field] });
  }
  return segments;
}

export function actionButtonVariant(style: ActionStyle): ButtonVariant {
  return style === "danger" ? "danger" : style === "secondary" ? "ghost" : "primary";
}

export interface ActionPresentation {
  label: string;
  style: ActionStyle;
}

export function actionStateFromQuery(result: unknown, declared: ActionPresentation): ActionPresentation {
  const r = result as Record<string, unknown> | undefined;
  if (r && typeof r === "object" && typeof r.label === "string") {
    return { label: r.label, style: (r.style as ActionStyle | undefined) ?? declared.style };
  }
  const id = mapQueryResultToVariantId(result);
  if (id === "update") return { label: "Update available", style: "primary" };
  if (id === "installed") return { label: "Reinstall", style: "secondary" };
  return declared;
}

export function SchemaActionBar({
  actions,
  disabled = false,
  remoteMachineName,
}: {
  actions: readonly ActionSetting[];
  disabled?: boolean;
  remoteMachineName?: string;
}) {
  const firstDanger = actions.findIndex((action) => action.style === "danger");
  return (
    <div className="space-y-2 pt-1">
      {disabled && remoteMachineName && (
        <RemoteLocalOnlyNotice machineName={remoteMachineName} subject="Harness setup actions" />
      )}
      <div role="group" aria-label="Actions" className="flex flex-wrap items-center gap-2" data-testid="settings-schema-actions">
        {actions.map((action, index) => (
          <ActionField key={action.id} action={action} trailing={index === firstDanger && index > 0} disabled={disabled} />
        ))}
      </div>
    </div>
  );
}

function ActionField({ action, trailing, disabled = false }: { action: ActionSetting; trailing: boolean; disabled?: boolean }) {
  const { label, command, state_query: stateQuery, args, confirm, style } = action;
  const [state, setState] = useState<ActionPresentation | null>(null);
  const [busy, setBusy] = useState(false);
  const [justDone, setJustDone] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const argsKey = JSON.stringify(args ?? null);

  const refreshState = useCallback(() => {
    if (!stateQuery || disabled) return;
    const parsed = argsKey === "null" ? undefined : (JSON.parse(argsKey) as Record<string, unknown>);
    invoke<unknown>(stateQuery, parsed)
      .then((result) => setState(actionStateFromQuery(result, { label, style })))
      .catch(() => setState({ label, style }));
  }, [stateQuery, argsKey, label, style, disabled]);

  useEffect(() => {
    if (stateQuery && !disabled) refreshState();
  }, [stateQuery, refreshState, disabled]);

  useEffect(() => {
    if (!justDone) return;
    const t = setTimeout(() => setJustDone(false), 2000);
    return () => clearTimeout(t);
  }, [justDone]);

  const click = useCallback(async () => {
    if (disabled) return;
    if (confirm && !window.confirm(confirm)) return;
    setBusy(true);
    setError(null);
    try {
      const parsed = argsKey === "null" ? undefined : (JSON.parse(argsKey) as Record<string, unknown>);
      await invoke(command, parsed);
      setJustDone(true);
      if (stateQuery) refreshState();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [command, argsKey, confirm, stateQuery, refreshState, disabled]);

  const effective = state ?? { label, style };
  return (
    <>
      <Button
        variant={actionButtonVariant(effective.style)}
        size="compact"
        data-testid="settings-schema-action"
        data-action-id={action.id}
        data-action-style={effective.style}
        onClick={click}
        loading={busy}
        disabled={disabled || justDone}
        className={trailing ? "gap-1.5 sm:ml-auto" : "gap-1.5"}
      >
        {justDone && <Check size={12} aria-hidden />}
        {justDone ? "Done" : effective.label}
      </Button>
      {error && <InlineErrorNotice error={error} operation="updating this setting" blocking />}
    </>
  );
}
