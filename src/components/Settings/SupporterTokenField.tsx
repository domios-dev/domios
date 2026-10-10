import { useRef, useState, type FormEvent } from "react";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";

interface Props {
  adapter: string;
  mode: "required" | "optional";
  hasToken: boolean;
  busy: boolean;
  saveToken: (adapter: string, token: string) => Promise<boolean>;
  showTrigger?: boolean;
  editing?: boolean;
  onEditingChange?: (editing: boolean) => void;
}

export function SupporterTokenField({
  adapter,
  mode,
  hasToken,
  busy,
  saveToken,
  showTrigger = true,
  editing: controlledEditing,
  onEditingChange,
}: Props) {
  const input = useRef<HTMLInputElement>(null);
  const [localEditing, setLocalEditing] = useState(false);
  const editing = controlledEditing ?? localEditing;
  const setEditing = (value: boolean) => {
    if (onEditingChange) onEditingChange(value);
    else setLocalEditing(value);
  };
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const token = input.current?.value.trim() ?? "";
    if (!token) return;
    const pending = saveToken(adapter, token);
    if (input.current) input.current.value = "";
    if (await pending) setEditing(false);
  };
  const id = `supporter-token-${adapter}`;
  return (
    <div className="inline-flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-[var(--ct-muted-fg)]">
      <span>{hasToken ? "Token saved, hidden" : "No token stored"}</span>
      {showTrigger && (
        <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => setEditing(true)}>
          {hasToken ? "Replace token" : "Add token"}
        </Button>
      )}
      {editing && (
        <form onSubmit={(event) => void submit(event)} className="flex flex-wrap items-center gap-1.5">
          <label htmlFor={id}>{mode === "optional" ? "API token (optional)" : "API token"}</label>
          <input
            id={id}
            ref={input}
            type="password"
            autoComplete="off"
            spellCheck={false}
            required
            autoFocus
            disabled={busy}
            className={UI_INPUT.compact}
          />
          <Button type="submit" variant="accent" size="sm" disabled={busy}>
            Save token
          </Button>
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => setEditing(false)}>
            Cancel
          </Button>
        </form>
      )}
    </div>
  );
}
