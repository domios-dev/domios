import { useEffect, useId, useRef, useState } from "react";
import { Lock, Minus, Plus, SlidersHorizontal } from "lucide-react";
import { BlockNotice } from "./BlockNotice";
import { BLOCK_INPUT, BlockButton, ChatBlockFrame, FOCUS_RING } from "./ChatBlockFrame";
import { useChatBlockHost, type SettingBlockHost } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";
import {
  SETTING_REFUSAL_TEXT,
  canStep,
  choiceLayout,
  isSensitiveKey,
  stepValue,
  type SettingBlockState,
  type SettingControl,
  type SettingInfo,
  type SettingOption,
  type SettingValue,
} from "./settingBlockModel";

const LOADING: SettingBlockState = { kind: "loading" };
const SENSITIVE: SettingBlockState = { kind: "refused", reason: "sensitive" };

/** `@ds-setting <config.key>`: one real setting inline, rendered and written through its settings schema. */
export function SettingBlock({ settingKey, source }: { settingKey: string; source: string }) {
  const host = useChatBlockHost().settings;
  const sensitive = isSensitiveKey(settingKey);
  const live = useLiveBinding(sensitive ? undefined : host?.binding, settingKey, LOADING);
  const state = sensitive ? SENSITIVE : live;
  if (state.kind === "refused") return <SettingNotice source={source} settingKey={settingKey} text={SETTING_REFUSAL_TEXT[state.reason]} refused />;
  if (!host) return <SettingNotice source={source} settingKey={settingKey} text="Settings are unavailable in this view." />;
  switch (state.kind) {
    case "loading":
      return <SettingNotice source={source} settingKey={settingKey} text="Reading the setting…" />;
    case "unavailable":
      return <SettingNotice source={source} settingKey={settingKey} text={`Setting unavailable: ${state.message}`} />;
    case "ready":
      return <SettingCard setting={state.setting} host={host} source={source} />;
  }
}

type WriteState = { pending: boolean; error: string | null };

function SettingCard({ setting, host, source }: { setting: SettingInfo; host: SettingBlockHost; source: string }) {
  const [write, setWrite] = useState<WriteState>({ pending: false, error: null });
  const inFlight = useRef(false);
  const commit = async (value: SettingValue) => {
    if (inFlight.current || value === setting.value) return;
    inFlight.current = true;
    setWrite({ pending: true, error: null });
    try {
      await host.write(setting.key, value);
      setWrite({ pending: false, error: null });
    } catch (error) {
      setWrite({ pending: false, error: `Could not save ${setting.label}: ${error instanceof Error ? error.message : String(error)}` });
    } finally {
      inFlight.current = false;
    }
  };
  const openSettings = host.openSettings;
  return (
    <ChatBlockFrame
      name="setting"
      source={source}
      title={setting.label}
      icon={<SlidersHorizontal size={14} aria-hidden="true" />}
      summary={setting.description}
      body={(
        <div className="flex min-w-0 flex-col gap-1.5" data-setting-key={setting.key} aria-busy={write.pending || undefined}>
          <SettingControlView setting={setting} disabled={write.pending} onCommit={(value) => void commit(value)} />
          {write.error && <p role="alert" className="m-0 text-xs" style={{ color: "var(--ct-err)" }}>{write.error}</p>}
        </div>
      )}
      details={(
        <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
          <dt>Key</dt><dd className="m-0 font-mono">{setting.key}</dd>
          {setting.owner && <><dt>Belongs to</dt><dd className="m-0">{setting.owner}</dd></>}
        </dl>
      )}
      fullPage={openSettings ? { label: setting.owner ? `Open ${setting.owner} settings` : "Open settings", open: () => openSettings(setting.key) } : undefined}
      askText={`About the ${setting.label} setting (\`${source}\`): `}
    />
  );
}

interface ControlProps {
  setting: SettingInfo;
  disabled: boolean;
  onCommit(value: SettingValue): void;
}

function SettingControlView({ setting, disabled, onCommit }: ControlProps) {
  const control: SettingControl = setting.control;
  switch (control.kind) {
    case "switch":
      return <SwitchControl label={setting.label} checked={setting.value === true} disabled={disabled} onToggle={() => onCommit(setting.value !== true)} />;
    case "choice":
      return choiceLayout(control.options) === "segmented"
        ? <SegmentedControl label={setting.label} options={control.options} value={String(setting.value)} disabled={disabled} onPick={onCommit} />
        : <SelectControl label={setting.label} options={control.options} value={String(setting.value)} disabled={disabled} onPick={onCommit} />;
    case "stepper":
      return <StepperControl label={setting.label} control={control} value={Number(setting.value)} disabled={disabled} onStep={onCommit} />;
    case "text":
      return <TextControl label={setting.label} value={String(setting.value)} placeholder={control.placeholder} disabled={disabled} onSave={onCommit} />;
  }
}

function SwitchControl({ label, checked, disabled, onToggle }: { label: string; checked: boolean; disabled: boolean; onToggle(): void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={onToggle}
      data-setting-control="switch"
      className={`inline-flex w-fit items-center gap-2 rounded-md text-xs disabled:opacity-50 ${FOCUS_RING}`}
    >
      <span
        aria-hidden="true"
        className="relative inline-flex h-4 w-7 shrink-0 items-center rounded-full transition-colors motion-reduce:transition-none"
        style={{ background: checked ? "var(--ct-accent)" : "color-mix(in srgb, var(--ct-muted-fg) 35%, transparent)" }}
      >
        <span
          className={`absolute h-3 w-3 rounded-full transition-transform motion-reduce:transition-none ${checked ? "translate-x-[13px]" : "translate-x-[2px]"}`}
          style={{ background: "var(--ct-bg)" }}
        />
      </span>
      <span style={{ color: "var(--ct-fg)" }}>{checked ? "On" : "Off"}</span>
    </button>
  );
}

function SegmentedControl({ label, options, value, disabled, onPick }: { label: string; options: SettingOption[]; value: string; disabled: boolean; onPick(value: string): void }) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      data-setting-control="segmented"
      className="ct-bar-pill-group w-fit max-w-full"
    >
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            disabled={disabled}
            onClick={() => onPick(option.value)}
            className="ct-bar-pill disabled:opacity-50"
            style={selected ? { background: "color-mix(in srgb, var(--ct-accent) 16%, transparent)", color: "var(--ct-fg)" } : undefined}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

function SelectControl({ label, options, value, disabled, onPick }: { label: string; options: SettingOption[]; value: string; disabled: boolean; onPick(value: string): void }) {
  return (
    <select
      aria-label={label}
      value={value}
      disabled={disabled}
      data-setting-control="select"
      onChange={(event) => onPick(event.currentTarget.value)}
      className={`${BLOCK_INPUT} w-fit max-w-full`}
    >
      {options.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
    </select>
  );
}

function StepperControl({ label, control, value, disabled, onStep }: {
  label: string;
  control: Extract<SettingControl, { kind: "stepper" }>;
  value: number;
  disabled: boolean;
  onStep(value: number): void;
}) {
  const range = [control.min !== undefined && `min ${control.min}`, control.max !== undefined && `max ${control.max}`].filter(Boolean).join(", ");
  const stepButton = (direction: 1 | -1) => (
    <button
      type="button"
      aria-label={`${direction > 0 ? "Increase" : "Decrease"} ${label}`}
      disabled={disabled || !canStep(value, direction, control)}
      onClick={() => onStep(stepValue(value, direction, control))}
      className="ct-bar-pill disabled:opacity-40"
      style={{ width: 28, justifyContent: "center", padding: 0 }}
    >
      {direction > 0 ? <Plus size={12} aria-hidden="true" /> : <Minus size={12} aria-hidden="true" />}
    </button>
  );
  return (
    <div className="flex items-center gap-2" data-setting-control="stepper">
      <div role="group" aria-label={label} className="ct-bar-pill-group items-center">
        {stepButton(-1)}
        <output className="min-w-[4ch] px-1 text-center text-sm font-semibold tabular-nums" style={{ color: "var(--ct-fg)" }}>{value}</output>
        {stepButton(1)}
      </div>
      {range && <span className="text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>{range}</span>}
    </div>
  );
}

function TextControl({ label, value, placeholder, disabled, onSave }: { label: string; value: string; placeholder?: string; disabled: boolean; onSave(value: string): void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const fieldId = useId();
  useEffect(() => setDraft(null), [value]);
  const shown = draft ?? value;
  const dirty = draft !== null && draft !== value;
  return (
    <div className="flex min-w-0 items-center gap-2" data-setting-control="text">
      <label htmlFor={fieldId} className="sr-only">{label}</label>
      <input
        id={fieldId}
        type="text"
        value={shown}
        placeholder={placeholder}
        disabled={disabled}
        spellCheck={false}
        onChange={(event) => setDraft(event.currentTarget.value)}
        className={`${BLOCK_INPUT} w-full max-w-[18rem] flex-1`}
      />
      <BlockButton primary disabled={disabled || !dirty} data-block-action="save" onClick={() => onSave(shown)}>Save</BlockButton>
    </div>
  );
}

function SettingNotice({ source, settingKey, text, refused = false }: { source: string; settingKey: string; text: string; refused?: boolean }) {
  return <BlockNotice name="setting" source={source} label={`Setting ${settingKey}`} text={text} icon={SlidersHorizontal} missing={refused} missingIcon={Lock} />;
}
