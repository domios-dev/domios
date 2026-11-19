import type { ProviderDescriptor } from "../../lib/types";
import { useProviderAccounts } from "../../hooks/useProviderAccounts";
import { ProviderAccountsCard } from "./ProviderAccountsCard";

export function ProviderAccountsSection({ provider }: { provider: ProviderDescriptor }) {
  const accounts=useProviderAccounts(provider.id);
  return <ProviderAccountsCard provider={provider} {...accounts} />;
}
