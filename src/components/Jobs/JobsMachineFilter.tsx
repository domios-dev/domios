import type { PairedPeer } from "../../lib/types";
import { UI_INPUT, UI_TEXT } from "../shared/design";
import { jobsMachineOptions } from "./jobsMachineModel";

export function JobsMachineFilter({ value, peers, onChange }: { value: string; peers: readonly PairedPeer[]; onChange: (value: string) => void }) {
  const options = jobsMachineOptions(peers);
  return <label className={`flex min-w-0 items-center gap-2 ${UI_TEXT.meta}`}>
    Machine
    <select aria-label="Machine" className={`${UI_INPUT.select} h-7 !rounded-full min-w-0 max-w-full`} value={value} onChange={(event) => onChange(event.target.value)}>
      {!options.some((option) => option.id === value) && <option value={value}>Unavailable machine</option>}
      {options.map((option) => <option key={option.id} value={option.id}>{option.name}{option.online ? "" : " · offline"}</option>)}
    </select>
  </label>;
}
