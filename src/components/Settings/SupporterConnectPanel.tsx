import { useRef, useState, type FormEvent } from "react";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";
import type { SupporterAdapter, SupporterMutation } from "./supporterTypes";

interface Props {
  adapters: SupporterAdapter[];
  busy: boolean;
  connect: (adapter: string, model: string, token?: string) => Promise<SupporterMutation | null>;
  finish: (adapter?: string, active?: boolean) => void;
}

export function SupporterConnectPanel({ adapters, busy, connect, finish }: Props) {
  const [adapterId, setAdapter] = useState("");
  const [model, setModel] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const adapter = adapters.find((entry) => entry.id === adapterId);
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!adapter || !model || busy) return;
    const token = adapter.token_mode === "none" ? undefined : input.current?.value.trim();
    if (adapter.token_mode === "required" && !token) return;
    const pending = connect(adapter.id, model, token || undefined);
    if (input.current) input.current.value = "";
    const result = await pending;
    if (result) finish(adapter.id, result.state === "active");
  };
  if (!adapters.length) {
    return (
      <div role="status" className="flex flex-wrap items-center gap-2 rounded-md border border-[var(--ct-border)] p-2 text-xs">
        <span>No other Spinal Cord adapters are installed. Install one from Plugins, then connect it here.</span>
        <Button variant="ghost" size="sm" onClick={() => finish()}>
          Close
        </Button>
      </div>
    );
  }
  return (
    <form
      onSubmit={(event) => void submit(event)}
      aria-label="Connect another adapter"
      className="flex flex-col gap-1.5 rounded-md border border-[var(--ct-border)] p-2"
    >
      <label className="text-xs text-[var(--ct-muted-fg)]" htmlFor="supporter-connect-adapter">Adapter</label>
      <select
        id="supporter-connect-adapter"
        value={adapterId}
        required
        autoFocus
        disabled={busy}
        className={UI_INPUT.fullCompact}
        onChange={(event) => {
          setAdapter(event.target.value);
          setModel("");
          if (input.current) input.current.value = "";
        }}
      >
        <option value="" disabled>
          Choose an adapter
        </option>
        {adapters.map((entry) => (
          <option key={entry.id} value={entry.id}>
            {entry.metadata?.display_name ?? entry.id}
          </option>
        ))}
      </select>
      {adapter && (
        <>
          <label className="text-xs text-[var(--ct-muted-fg)]" htmlFor="supporter-connect-model">Model</label>
          <select
            id="supporter-connect-model"
            value={model}
            required
            disabled={busy}
            className={UI_INPUT.fullCompact}
            onChange={(event) => setModel(event.target.value)}
          >
            <option value="" disabled>
              Choose a model
            </option>
            {adapter.models.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.display_name}
              </option>
            ))}
          </select>
          {adapter.token_mode !== "none" && (
            <>
              <label className="text-xs text-[var(--ct-muted-fg)]" htmlFor="supporter-connect-token">
                {adapter.token_mode === "required" ? "API token" : "API token (optional)"}
              </label>
              <input
                id="supporter-connect-token"
                key={adapter.id}
                ref={input}
                type="password"
                required={adapter.token_mode === "required"}
                disabled={busy}
                autoComplete="off"
                spellCheck={false}
                className={UI_INPUT.fullCompact}
              />
            </>
          )}
        </>
      )}
      <div className="flex gap-2">
        <Button type="submit" variant="accent" size="sm" disabled={busy || !adapter || !model}>
          Connect &amp; make active
        </Button>
        <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => finish()}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
