import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "../lib/bridge";
import { selectedDaemonTransport } from "../lib/daemonRuntime";
import type { Tab } from "../lib/types";
import { useAgentProviders } from "./useAgentProviders";
import { useProviderAccounts } from "./useProviderAccounts";
import type { ProviderAccountsSnapshot } from "./providerAccountsTypes";
import { useAccountTransition } from "./useAccountTransition";

interface WorkspaceProvider {
  providerId: string;
  displayName: string;
  snapshot: ProviderAccountsSnapshot;
}

export function useWorkspaceAccounts(workspaceId: string) {
  const catalogue = useAgentProviders();
  const capable = useMemo(() => catalogue.providers.filter(provider => provider.accounts), [catalogue.providers]);
  const [providers, setProviders] = useState<WorkspaceProvider[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const mutation = useRef(false);
  useEffect(() => {
    let active = true;
    let queued = false;
    let reading = false;
    const controller = new AbortController();
    const read = async () => {
      if (reading) { queued = true; return; }
      reading = true;
      const sequence = ++generation.current;
      try {
        const next: WorkspaceProvider[] = [];
        for (const provider of capable) {
          const snapshot = await selectedDaemonTransport.json<ProviderAccountsSnapshot>("GET", `/api/agents/providers/${encodeURIComponent(provider.id)}/accounts`, undefined, { signal: controller.signal });
          if (snapshot.providerId !== provider.id) throw new Error("The account response does not match this harness");
          if (snapshot.accounts.length > 1) next.push({ providerId: provider.id, displayName: provider.display_name, snapshot });
        }
        if (active && sequence === generation.current) { setProviders(next); setError(null); }
      } catch (failure) { if (active && !controller.signal.aborted) setError(String(failure)); }
      finally {
        reading = false;
        if (active) setLoading(false);
        if (active && queued) { queued = false; void read(); }
      }
    };
    setProviders([]);
    setLoading(true);
    let stop: (() => void) | undefined;
    void listen("provider-accounts-changed", () => { void read(); }).then(unlisten => {
      if (!active) { unlisten(); return; }
      stop = unlisten;
      void read();
    }).catch(failure => { if (active) { setError(String(failure)); setLoading(false); } });
    return () => { active = false; generation.current++; controller.abort(); stop?.(); };
  }, [capable, workspaceId]);
  const choose = useCallback(async (providerId: string, accountId: string | null) => {
    const rendered = providers.find(provider => provider.providerId === providerId);
    if (!rendered) throw new Error("The account list is unavailable");
    if (mutation.current) throw new Error("Another account change is in progress");
    mutation.current = true;
    const sequence = generation.current;
    try {
      const snapshot = await selectedDaemonTransport.json<ProviderAccountsSnapshot>("POST", `/api/agents/providers/${encodeURIComponent(providerId)}/accounts`, {
        revision: rendered.snapshot.revision, operation: { action: "workspace", workspaceId, accountId },
      });
      if (snapshot.providerId !== providerId) throw new Error("The account response does not match this harness");
      if (sequence === generation.current) setProviders(current => current.map(provider => provider.providerId === providerId ? { ...provider, snapshot } : provider));
    } catch (failure) { if (sequence === generation.current) setError(String(failure)); throw failure; }
    finally { mutation.current = false; }
  }, [providers, workspaceId]);
  return { providers, loading: loading || catalogue.loading, error: error ?? catalogue.error, choose };
}

export function useSpawnAccounts(providerId: string | null, workspaceId: string) {
  const catalogue=useAgentProviders();
  const capable=Boolean(catalogue.providers.find(provider=>provider.id===providerId)?.accounts);
  const accounts = useProviderAccounts(capable ? providerId : null);
  const [choice, setChoice] = useState<{ provider: string; workspace: string; account: string } | null>(null);
  const selected = choice?.provider === providerId && choice.workspace === workspaceId ? choice.account : null;
  const available=accounts.snapshot?.accounts.filter(account=>account.status!=="removing") ?? [];
  const override=accounts.snapshot?.workspaceOverrides.find(override=>override.workspaceId===workspaceId)?.accountId;
  const starting=available.find(account=>account.id===override)?.id ?? available[0]?.id;
  const explicitAccountId = available.some(account => account.id === selected) ? selected : null;
  const selectedAccountId = explicitAccountId ?? starting ?? null;
  const select = useCallback((accountId: string) => {
    if (!providerId || !accounts.snapshot?.accounts.some(account => account.id === accountId && account.status !== "removing")) return;
    setChoice({ provider: providerId, workspace: workspaceId, account: accountId });
  }, [providerId, workspaceId, accounts.snapshot]);
  return { snapshot: accounts.snapshot, selectedAccountId, explicitAccountId, select, loading: catalogue.loading || accounts.loading, error: accounts.error ?? catalogue.error };
}

export function useTabAccounts(tab: Tab) {
  const catalogue = useAgentProviders();
  const descriptor = catalogue.providers.find(provider => provider.id === tab.agent_type);
  const accounts = useProviderAccounts(descriptor?.accounts && !tab.remote_peer_id ? tab.agent_type ?? null : null);
  const transition=useAccountTransition(descriptor?.accounts && !tab.remote_peer_id ? tab.id:null,tab.agent_type ?? null,`${tab.session_binding_nonce}:${tab.agent_runtime_state?.epoch}:${tab.agent_runtime_state?.active_turn_started_at_ms}`);
  const turnActive=transition.turnActive || tab.agent_runtime_state?.active_turn_started_at_ms!=null;
  useEffect(()=>{if(accounts.snapshot)transition.onRefresh();},[accounts.snapshot?.revision,transition.onRefresh]);
  const currentAccountId = tab.account?.id ?? "default";
  const [pending, setPending] = useState<string | null>(null);
  const inFlight=useRef(false);
  const [failure, setFailure] = useState<{binding:string;reason:string} | null>(null);
  const binding = `${tab.id}:${tab.agent_type}:${tab.session_binding_nonce}:${tab.agent_runtime_state?.epoch}:${currentAccountId}:${tab.account?.namespace}`;
  const latest = useRef(binding);
  latest.current = binding;
  const blockedReason = accounts.snapshot && accounts.snapshot.continuation !== "proven"
    ? accounts.snapshot.continuation === "unsupported"
      ? `${descriptor?.display_name ?? "This harness"} can't move a running conversation to another account.`
      : "Moving this conversation to another account isn't confirmed yet."
    : null;
  const choose = useCallback(async (accountId: string,resume?:{turnId:string}) => {
    const snapshot = accounts.snapshot;
    const verifying=snapshot?.continuation==="unverified" && transition.canVerify && !turnActive && !resume;
    if (!snapshot || latest.current !== binding || (blockedReason && !verifying) || inFlight.current) throw new Error(blockedReason ?? "The tab account changed. Refresh it before trying again.");
    if (accountId === currentAccountId) return;
    const target = snapshot.accounts.find(account => account.id === accountId);
    if (!target || target.status === "signedOut" || target.status === "removing" || (verifying && target.status!=="ready")) throw new Error("The selected account is unavailable");
    inFlight.current=true;
    setPending(binding);
    setFailure(null);
    try {
      await selectedDaemonTransport.json("POST", `/api/tabs/${encodeURIComponent(tab.id)}/account${verifying?"/verify":""}`, {
        providerId: tab.agent_type, accountId, previousAccountId: currentAccountId,
        accountNamespace:tab.account?.namespace ?? null,
        ...(resume?{resumeWork:true,turnId:resume.turnId}:{}),
        nonce: tab.session_binding_nonce, epoch: tab.agent_runtime_state?.epoch ?? null, revision: snapshot.revision,
      });
    } catch (failure) { if (latest.current === binding) setFailure({binding,reason:String(failure)}); throw failure; }
    finally { inFlight.current=false;setPending(current => current === binding ? null : current); }
  }, [accounts.snapshot, binding, blockedReason, pending, currentAccountId, tab.id, tab.agent_type, tab.session_binding_nonce, tab.agent_runtime_state?.epoch,tab.account?.namespace,transition.canVerify,turnActive]);
  return { canVerify:transition.canVerify,turnActive,snapshot: accounts.snapshot, currentAccountId, transition:transition.transition,switching: pending === binding || transition.transition?.status==="waiting" || transition.transition?.status==="switching", blockedReason, error: (failure?.binding===binding?failure.reason:null) ?? accounts.error ?? transition.error, choose,continueOn:(accountId:string,turnId:string)=>choose(accountId,{turnId}) };
}
