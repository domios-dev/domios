import type { Notice } from "../../lib/notices";
import { describeError } from "../../lib/describeError";
import type { SignInFailure } from "./accountModel";
import { SubscriptionServiceUnavailable } from "./accountPlanModel";
import { signInFailureToastMessage } from "./useAccount";



function copyDetailsAction(raw: string) {
  return {
    label: "Copy details",
    run: async () => {
      try {
        await navigator.clipboard.writeText(raw);
      } catch {
        return;
      }
    },
  };
}

export function signInFailureToast(failure: SignInFailure, retry: () => void | Promise<void>): Notice {
  const details = describeError(failure, { operation: "signing in" });
  return { kind: "toast", id: "account-sign-in-failure",
    title: "Sign-in could not be completed",
    detail: signInFailureToastMessage(failure),
    severity: "Warning",
    tabId: null,
    workspaceId: null,
    actions: [copyDetailsAction(details.raw), { label: "Retry", run: retry }] };
}

export function billingFailureToast(
  error: unknown,
  operation: string,
  planStatus: string | undefined,
  retry: () => void | Promise<void>,
): Notice {
  const details = describeError(error, { operation });
  if (error instanceof SubscriptionServiceUnavailable) return { kind: "toast", id: "account-billing-action-failure",
    title: "Subscription service unavailable",
    detail: "The subscription service is unavailable right now. Try again later.",
    severity: "Warning",
    tabId: null,
    workspaceId: null,
    actions: [...(details.raw ? [copyDetailsAction(details.raw)] : []), { label: "Retry", run: retry }] };
  const normalized = details.raw.toLowerCase().replace(/[_-]+/g, " ");
  const unavailable = planStatus === "unavailable" ||
    /billing.{0,40}unavailable|subscription.{0,40}unavailable|functions? url|billing.{0,40}not configured/.test(normalized);
  return { kind: "toast", id: "account-billing-action-failure",
    title: unavailable ? "Subscription management unavailable" : details.title,
    detail: unavailable ? "Subscription management is unavailable in this build." : details.cause,
    severity: "Warning",
    tabId: null,
    workspaceId: null,
    actions: [
      ...(details.raw ? [copyDetailsAction(details.raw)] : []),
      ...(!unavailable ? [{ label: "Retry", run: retry }] : []),
    ] };
}
