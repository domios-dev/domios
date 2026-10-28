import { useEffect, useRef, useState } from "react";
import { billingOperation, parseDesktopCatalog, parseSubscription, type SubscriptionReadback } from "./desktopBilling";
import type { PlanCatalog } from "./planCatalog";
import { SharedRead } from "../../lib/sharedRead";

const catalogRead = new SharedRead<PlanCatalog>();
const subscriptions = new Map<string, SharedRead<SubscriptionReadback>>();
function subscriptionRead(identity: string) {
  let read = subscriptions.get(identity);
  if (!read) { read = new SharedRead<SubscriptionReadback>(); subscriptions.set(identity, read); }
  return read;
}

interface LoadedBilling {
  identity: string | null;
  catalog: PlanCatalog | null;
  subscription: SubscriptionReadback | null;
  error: string | null;
  settled: boolean;
}

export function useDesktopBilling(identity: string | null, enabled = true) {
  const [loaded, setLoaded] = useState<LoadedBilling>({ identity, catalog: null, subscription: null, error: null, settled: false });
  const [revision, setRevision] = useState(0);
  const [changing, setChanging] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const busy = useRef(false);
  const currentIdentity = useRef(identity);
  currentIdentity.current = identity;
  const generation = useRef(0);
  const lastRevision = useRef(0);
  const targetTier = useRef<string | null>(null);
  const changeNotice = useRef<string | null>(null);

  useEffect(() => { setSubmitted(false); targetTier.current = null; changeNotice.current = null; }, [identity]);

  useEffect(() => {
    const epoch = ++generation.current;
    const controller = new AbortController();
    if (!identity) subscriptions.clear();
    if (!identity || !enabled) {
      setLoaded({ identity, catalog: null, subscription: null, error: null, settled: true });
      return () => controller.abort();
    }
    const read = subscriptionRead(identity);
    const unsubscribe = read.subscribe(subscription => {
      if (!controller.signal.aborted && generation.current === epoch) setLoaded(previous => ({ ...previous, identity, subscription }));
    });
    let latest = 0;
    async function load(force = false) {
      const request = ++latest;
      const [catalog, subscription] = await Promise.allSettled([
        catalogRead.read(() => billingOperation("catalog").then(parseDesktopCatalog), force),
        read.read(() => billingOperation("subscription").then(parseSubscription), force),
      ]);
      if (controller.signal.aborted || generation.current !== epoch || request !== latest) return;
      if (subscription.status === "fulfilled" && targetTier.current === subscription.value.current_tier && !subscription.value.pending_change) {
        setSubmitted(false);
        targetTier.current = null;
        changeNotice.current = null;
      }
      setLoaded({ identity, settled: true,
        catalog: catalog.status === "fulfilled" ? catalog.value : null,
        subscription: subscription.status === "fulfilled" ? subscription.value : null,
        error: catalog.status === "rejected" ? "Current plan offers are unavailable. Refresh to try again."
          : subscription.status === "rejected" ? "Your subscription status is unavailable. Refresh before choosing a plan." : changeNotice.current,
      });
    }
    const force = revision !== lastRevision.current;
    lastRevision.current = revision;
    void load(force);
    const onFocus = () => { void load(); };
    window.addEventListener("focus", onFocus);
    return () => { unsubscribe(); controller.abort(); generation.current++; window.removeEventListener("focus", onFocus); };
  }, [identity, revision, enabled]);

  async function change(priceId: string): Promise<boolean> {
    if (!identity || busy.current || submitted) return false;
    busy.current = true;
    setChanging(true);
    setSubmitted(true);
    changeNotice.current = null;
    try {
      const response = await billingOperation("change", priceId);
      if (currentIdentity.current !== identity) return false;
      const value = response as { status?: unknown; target_plan?: unknown } | null;
      if (!value || !["scheduled", "awaiting_webhook"].includes(String(value.status))) throw new Error("Unconfirmed plan change");
      targetTier.current = typeof value.target_plan === "string" ? value.target_plan : null;
      const subscription = await subscriptionRead(identity).read(() => billingOperation("subscription").then(parseSubscription), true);
      if (currentIdentity.current !== identity) return false;
      changeNotice.current = value.status === "awaiting_webhook" ? "Your upgrade is awaiting payment confirmation." : null;
      setLoaded(previous => ({ ...previous, subscription, error: changeNotice.current }));
      return true;
    } catch (failure: unknown) {
      const message = failure instanceof Error ? failure.message : "";
      const refused = /^(test_billing_restricted|renewal_window|interval_change_unavailable|subscription_unavailable|same_tier): (.+)$/.exec(message);
      const pending = /^(confirmation_pending|change_pending): (.+)$/.exec(message);
      if (currentIdentity.current === identity) {
        if (refused) setSubmitted(false);
        changeNotice.current = refused?.[2] ?? pending?.[2] ?? "Your plan change could not be confirmed. Refresh its status before submitting another change.";
        setLoaded(previous => ({ ...previous, error: changeNotice.current }));
      }
      return false;
    } finally {
      busy.current = false;
      setChanging(false);
    }
  }

  const visible = loaded.identity === identity ? loaded : null;
  return { catalog: visible?.catalog ?? null, subscription: visible?.subscription ?? null,
    error: visible?.error ?? null, loading: identity !== null && !visible?.settled,
    changing, submitted, change, refresh: () => setRevision(value => value + 1) };
}
