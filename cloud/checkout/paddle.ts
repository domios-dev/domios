export interface PaddleCheckoutConfig {
  paddleEnvironment: "sandbox" | "production";
  paddleClientToken: string;
}
export interface PaddleCheckoutCallbacks {
  onCompleted?(transactionId: string): void;
  onClosed?(): void;
  onError?(error: Error): void;
}
export interface PaddleCheckoutEvent { name: string; data?: {transaction_id?: string; error?: {detail?: string; message?: string}} }
export function checkoutEventHandler(transactionId: string, callbacks: PaddleCheckoutCallbacks): (event: PaddleCheckoutEvent) => void {
  let completed = false;
  let closed = false;
  return event => {
    if (event.name === "checkout.completed") {
      if (completed || event.data?.transaction_id !== transactionId) return;
      completed = true;
      callbacks.onCompleted?.(transactionId);
    } else if (event.name === "checkout.closed") {
      if (completed || closed) return;
      closed = true;
      callbacks.onClosed?.();
    } else if (event.name === "checkout.error" || event.name === "checkout.payment.error") {
      callbacks.onError?.(new Error(event.data?.error?.detail ?? event.data?.error?.message ?? "Paddle checkout failed"));
    }
  };
}
export interface PaddleClient {
  Environment: {set(environment: "sandbox" | "production"): void};
  Initialize(options: {token: string; eventCallback: (event: PaddleCheckoutEvent) => void}): void;
  Checkout: {open(options: {transactionId: string}): void};
}
declare global { interface Window { Paddle?: PaddleClient } }
export const PADDLE_SCRIPT_URL = "https://cdn.paddle.com/paddle/v2/paddle.js";
let loading: Promise<PaddleClient> | undefined;
function loadPaddle(): Promise<PaddleClient> {
  if (window.Paddle) return Promise.resolve(window.Paddle);
  return loading ??= new Promise<PaddleClient>((resolve,reject) => {
    const script = document.createElement("script");
    script.src = PADDLE_SCRIPT_URL;
    script.async = true;
    script.addEventListener("load",() => window.Paddle ? resolve(window.Paddle) : reject(new Error("Paddle.js unavailable")),{once:true});
    script.addEventListener("error",() => reject(new Error("Paddle.js failed to load")),{once:true});
    document.head.append(script);
  }).catch(error => { loading = undefined; throw error; });
}
export function checkoutTransactionId(url: string, transactionId?: string): string {
  const id = transactionId ?? new URL(url).searchParams.get("_ptxn");
  if (!id || !/^txn_[a-z0-9]{26}$/.test(id)) throw new Error("A server-created checkout transaction is required");
  return id;
}
export async function openPaddleCheckout(config: PaddleCheckoutConfig, transactionId?: string,
  callbacks: PaddleCheckoutCallbacks = {}, url = window.location.href, loader: () => Promise<PaddleClient> = loadPaddle): Promise<void> {
  const id = checkoutTransactionId(url,transactionId);
  if (!["sandbox","production"].includes(config.paddleEnvironment)
    || !config.paddleClientToken.startsWith(config.paddleEnvironment === "sandbox" ? "test_" : "live_")) throw new Error("Invalid public checkout configuration");
  const paddle = await loader();
  paddle.Environment.set(config.paddleEnvironment);
  paddle.Initialize({token:config.paddleClientToken,eventCallback:checkoutEventHandler(id,callbacks)});
  paddle.Checkout.open({transactionId:id});
}
