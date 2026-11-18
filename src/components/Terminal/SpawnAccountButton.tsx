import type { ProviderAccountsSnapshot } from "../../hooks/providerAccountsTypes";
import { DropdownSelect, TOUCH_SELECT_HEIGHT } from "../shared/DropdownMenu";
import { AccountAvatar } from "../shared/AccountAvatar";
import { accountChoiceDescription, accountChoices, accountIdentity } from "../shared/accountChoices";

export function SpawnAccountButton({ snapshot, providerName, selectedAccountId, disabled, onSelect }: {
  snapshot: ProviderAccountsSnapshot | null;
  providerName: string;
  selectedAccountId: string | null;
  disabled?: boolean;
  onSelect: (accountId: string) => void;
}) {
  const accounts = snapshot ? accountChoices(snapshot) : [];
  if (accounts.length < 2) return null;
  const selected = accounts.find(account => account.id === selectedAccountId) ?? accounts[0];
  return <DropdownSelect aria-label="Account" value={selected.id} onChange={onSelect} disabled={disabled} iconOnly
    leading={<AccountAvatar account={selected} size={20} other={selected.id !== accounts[0].id} />}
    className={`${TOUCH_SELECT_HEIGHT} !w-9 pointer-coarse:!w-11 !px-0 justify-center`} menuMaxHeight={320}
    options={accounts.map(account => ({ value: account.id, label: accountIdentity(account, providerName), description: accountChoiceDescription(account) }))} />;
}
