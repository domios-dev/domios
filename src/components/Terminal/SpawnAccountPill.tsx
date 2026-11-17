import type { ProviderAccountsSnapshot } from "../../hooks/providerAccountsTypes";
import { DropdownSelect } from "../shared/DropdownMenu";
import { accountChoiceDescription, accountChoiceName, accountChoices } from "../shared/accountChoices";

export function SpawnAccountPill({ snapshot, providerName, selectedAccountId, disabled, onSelect }: {
  snapshot: ProviderAccountsSnapshot | null;
  providerName: string;
  selectedAccountId: string | null;
  disabled?: boolean;
  onSelect: (accountId: string) => void;
}) {
  const accounts = snapshot ? accountChoices(snapshot) : [];
  if (accounts.length < 2) return null;
  return <DropdownSelect aria-label="Account" value={selectedAccountId ?? ""} onChange={onSelect} disabled={disabled}
    className="min-h-11 sm:min-h-9 !w-auto max-w-full" menuMaxHeight={320}
    options={accounts.map(account => ({ value: account.id, label: accountChoiceName(account, providerName), description: accountChoiceDescription(account) }))} />;
}
