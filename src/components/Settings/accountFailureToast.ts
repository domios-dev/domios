import type { Notification } from "../../lib/notificationTypes";
import { describeError } from "../../lib/describeError";
import type { SignInFailure } from "./accountModel";
import { signInFailureToastMessage } from "./useAccount";

type ToastInput = Omit<Notification, "timestamp" | "read">;

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

export function signInFailureToast(failure: SignInFailure, retry: () => void | Promise<void>): ToastInput {
  const details = describeError(failure, { operation: "signing in" });
  return {
    id: "account-sign-in-failure",
    title: "Sign-in could not be completed",
    body: signInFailureToastMessage(failure),
    severity: "Warning",
    level: "action_needed",
    tab_id: null,
    workspace_id: null,
    local_actions: [copyDetailsAction(details.raw), { label: "Retry", run: retry }],
  };
}

export function billingFailureToast(
  error: unknown,
  operation: string,
  planStatus: string | undefined,
  retry: () => void | Promise<void>,
): ToastInput {
  const details = describeError(error, { operation });
  const normalized = details.raw.toLowerCase().replace(/[_-]+/g, " ");
  const unavailable = planStatus === "unavailable" ||
    /billing.{0,40}unavailable|subscription.{0,40}unavailable|functions? url|billing.{0,40}not configured/.test(normalized);
  return {
    id: "account-billing-action-failure",
    title: unavailable ? "Subscription management unavailable" : details.title,
    body: unavailable ? "Subscription management is unavailable in this build." : details.cause,
    severity: "Warning",
    level: "action_needed",
    tab_id: null,
    workspace_id: null,
    local_actions: [
      ...(details.raw ? [copyDetailsAction(details.raw)] : []),
      ...(!unavailable ? [{ label: "Retry", run: retry }] : []),
    ],
  };
}
