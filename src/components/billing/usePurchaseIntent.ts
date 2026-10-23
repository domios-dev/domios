import { useEffect, useRef, useState } from "react";
import type { Notice } from "../../lib/notices";
import { billingFailureToast, signInFailureToast } from "../Settings/accountFailureToast";
import type { AccountModel, LoginHint, SignInFailure } from "../Settings/accountModel";
import type { PlanModel } from "../Settings/accountPlanModel";
import type { PlanLoad } from "../Settings/useAccountPlan";
import type { PlanCatalog } from "./planCatalog";
import { AlreadySubscribedError } from "./freeLimitGate";
import { IDLE_INTENT, purchaseIntent, type PurchaseAccount, type PurchaseEvent, type PurchaseIntent } from "./purchaseIntent";

export interface PurchaseIntentPorts {
  account: { model: AccountModel; signIn: (hint?: LoginHint) => Promise<SignInFailure | null> };
  billing: { plan: PlanModel | null; planLoad: PlanLoad; upgrade: (planKey: string) => Promise<unknown | null> };
  catalog: PlanCatalog;
  notify: (notice: Notice) => void;
  /** Retry for sign-in failures this surface did not start for a purchase; without it they stay with their owner. */
  retrySignIn?: () => void;
  onCheckoutOpened?: () => void;
  /** Checkout was refused because a subscription already exists; the surface switches to Manage subscription. */
  onAlreadySubscribed?: () => void;
}

const CHECKOUT_OPERATION = "changing your subscription";

function purchaseAccount(model: AccountModel): PurchaseAccount {
  return model.kind === "signed_in" ? "signed_in" : model.kind === "pending" ? "pending" : "signed_out";
}

const failureKey = (failure: SignInFailure) => `${failure.stage}:${failure.error}`;

const planUnknownToast = (retry: () => void) =>
  billingFailureToast(new Error("Domios could not check your current plan."), CHECKOUT_OPERATION, undefined, retry);

/** The chosen plan's journey through sign-in to checkout; it lives and dies with the surface that owns the cards. */
export function usePurchaseIntent(ports: PurchaseIntentPorts) {
  const latest = useRef(ports);
  latest.current = ports;
  const [intent, setIntent] = useState<PurchaseIntent>(IDLE_INTENT);
  const current = useRef(intent);
  const mounted = useRef(false);
  const shownFailure = useRef<string | null>(null);
  const abandonedSignIn = useRef(false);

  function dispatch(event: PurchaseEvent): PurchaseIntent {
    const next = purchaseIntent(current.current, event);
    current.current = next;
    setIntent(next);
    return next;
  }

  async function openCheckout(planKey: string) {
    const failure = await latest.current.billing.upgrade(planKey);
    if (!mounted.current || current.current.kind !== "opening" || current.current.planKey !== planKey) return;
    dispatch({ type: "checkoutSettled" });
    const { billing, notify, onCheckoutOpened, onAlreadySubscribed } = latest.current;
    if (failure instanceof AlreadySubscribedError && onAlreadySubscribed) onAlreadySubscribed();
    else if (failure) notify(billingFailureToast(failure, CHECKOUT_OPERATION, billing.plan?.status, () => choose(planKey)));
    else onCheckoutOpened?.();
  }

  function choose(planKey: string) {
    const { account, billing, catalog } = latest.current;
    const before = current.current;
    const signedIn = purchaseAccount(account.model) === "signed_in";
    const next = dispatch({ type: "choose", planKey, account: purchaseAccount(account.model), plan: billing.plan, catalog });
    if (next === before) {
      if (signedIn && !billing.plan && before.kind === "idle") latest.current.notify(planUnknownToast(() => choose(planKey)));
      return;
    }
    abandonedSignIn.current = false;
    if (next.kind === "opening") void openCheckout(planKey);
    else if (next.kind === "signing_in" && !next.attached) void account.signIn();
  }

  function cancel() {
    if (current.current.kind === "signing_in" && latest.current.account.model.kind === "pending") abandonedSignIn.current = true;
    dispatch({ type: "cancel" });
  }

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const { model } = ports.account;
  const { plan, planLoad } = ports.billing;
  useEffect(() => {
    const waiting = current.current;
    if (waiting.kind !== "signing_in" || model.kind === "pending") return;
    const { catalog, notify } = latest.current;
    const retry = () => choose(waiting.planKey);
    if (model.kind === "signed_in") {
      if (planLoad === "loading") return;
      const next = dispatch({ type: "signedIn", plan, catalog });
      if (next.kind === "opening") void openCheckout(next.planKey);
      else if (next.kind === "idle") notify(planUnknownToast(retry));
      return;
    }
    dispatch({ type: "signInFailed" });
    if (model.kind === "error" && model.failure) {
      shownFailure.current = failureKey(model.failure);
      notify(signInFailureToast(model.failure, retry));
    }
  }, [intent, model, plan, planLoad]);

  useEffect(() => {
    if (model.kind !== "error" || !model.failure) {
      shownFailure.current = null;
      if (model.kind !== "pending") abandonedSignIn.current = false;
      return;
    }
    const key = failureKey(model.failure);
    if (shownFailure.current === key) return;
    shownFailure.current = key;
    if (abandonedSignIn.current) {
      abandonedSignIn.current = false;
      return;
    }
    const { notify, retrySignIn } = latest.current;
    if (retrySignIn) notify(signInFailureToast(model.failure, retrySignIn));
  }, [model]);

  return { intent, busy: intent.kind === "signing_in" || intent.kind === "opening", choose, cancel };
}
