import type { UserSummary } from "./model";

export interface PageFilters { provider: string; disabled: string; profile: string }
export function matchesPageFilters(user: UserSummary, filters: PageFilters): boolean {
  const provider = filters.provider;
  return (!provider || (provider === "anonymous" ? user.providers.length === 0
    : user.providers.includes(provider === "email" ? "password" : `${provider}.com`)))
    && (!filters.disabled || user.disabled === (filters.disabled === "yes"))
    && (!filters.profile || user.hasProfile === (filters.profile === "yes"));
}

export function nextPageStack(tokens: string[], nextToken: string | undefined): string[] {
  return nextToken ? [...tokens, nextToken] : tokens;
}
export function previousPageStack(tokens: string[]): string[] { return tokens.slice(0, -1); }
