import type { PairedPeer } from "../../lib/types";
import { DropdownSelect } from "../shared/DropdownMenu";
import { jobsMachineOptions } from "./jobsMachineModel";

export function JobsMachineFilter({ value, peers, onChange }: { value: string; peers: readonly PairedPeer[]; onChange: (value: string) => void }) {
  const options = jobsMachineOptions(peers);
  return <DropdownSelect aria-label="Machine" prefix="Machine" data-testid="jobs-machine" className="min-w-0 max-w-full" value={value} onChange={onChange}
    options={[
      ...(options.some((option) => option.id === value) ? [] : [{ value, label: "Unavailable machine" }]),
      ...options.map((option) => ({ value: option.id, label: option.name, ...(option.online ? {} : { badge: "offline" }) })),
    ]} />;
}
