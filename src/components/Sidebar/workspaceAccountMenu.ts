import type { useWorkspaceAccounts } from "../../hooks/useAccountSelectors";
import type { DropdownMenuItem } from "../shared/DropdownMenu";
import { accountChoiceDescription, accountChoiceName, accountChoices } from "../shared/accountChoices";

export function workspaceAccountMenu(workspaceId: string, providers: ReturnType<typeof useWorkspaceAccounts>["providers"],
  choose: (providerId: string, accountId: string | null) => void): DropdownMenuItem | null {
  const groups = providers.filter(provider => accountChoices(provider.snapshot).length > 1).map(provider => {
    const selected = provider.snapshot.workspaceOverrides.find(override => override.workspaceId === workspaceId)?.accountId ?? null;
    return { label: provider.displayName, groupKey: provider.providerId, items: [
      { label: "First available", selected: selected === null, onClick: () => choose(provider.providerId, null) },
      ...accountChoices(provider.snapshot).map(account => ({
        label: accountChoiceName(account, provider.displayName), description: accountChoiceDescription(account),
        selected: selected === account.id, onClick: () => choose(provider.providerId, account.id),
      })),
    ] };
  });
  return groups.length ? { label: "Accounts", submenu: { label: "Accounts", items: groups } } : null;
}
