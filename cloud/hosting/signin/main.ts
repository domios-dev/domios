import { signinHint, hintedAction } from "./hints";
import { callbackURL, challengeFromQuery } from "../../shared/protocol";

import { authClient, type AuthConfig } from "./adapters";
const status = document.querySelector<HTMLParagraphElement>("#status")!;
const providers = document.querySelector<HTMLDivElement>("#providers")!;
const email = document.querySelector<HTMLInputElement>("#email")!;
const buttons = document.querySelectorAll<HTMLButtonElement>("button");
let busy = false;

async function run(action: () => Promise<void>): Promise<void> {
  if (busy) return;
  busy = true;
  buttons.forEach((button) => { button.disabled = true; });
  try { await action(); }
  catch { status.textContent = "Sign-in could not be completed. Try again or start a new sign-in from Domios."; }
  finally {
    busy = false;
    buttons.forEach((button) => { button.disabled = false; });
  }
}

async function start(): Promise<void> {
  const query = new URLSearchParams(location.search);
  const challenge = challengeFromQuery(query);
  const hint = signinHint(query);
  const heading = hint.mode === "signup" ? "Create a Domios account" : "Sign in to Domios";
  document.querySelector("h1")!.textContent = heading;
  document.title = heading;
  const response = await fetch("config.json", {cache: "no-store"});
  if (!response.ok) throw new Error("Missing configuration");
  const config = await response.json() as AuthConfig;
  const auth = await authClient(config);
  const complete = async () => {
    const code = await auth.createCode(challenge);
    const target = callbackURL(challenge, code);
    await auth.signOut();
    sessionStorage.removeItem("domios-signin-email");
    history.replaceState(null, "", location.pathname);
    status.textContent = "Returning to Domios…";
    location.replace(target);
  };
  const emailLink = auth.isEmailLink();
  const finishEmail = async () => {
    await auth.finishEmail(email.value);
    await complete();
  };
  if (emailLink) {
    document.querySelector<HTMLButtonElement>("#google")!.hidden = true;
    document.querySelector<HTMLButtonElement>("#facebook")!.hidden = true;
    document.querySelector<HTMLButtonElement>("#email-submit")!.textContent = "Complete email sign-in";
    email.value = sessionStorage.getItem("domios-signin-email") ?? "";
  }
  for (const id of ["google", "facebook", "apple", "local"] as const) {
    const button = document.querySelector<HTMLButtonElement>(`#${id}`)!;
    button.hidden = emailLink || !auth.methods.includes(id);
    button.addEventListener("click", () => void run(async () => {
      await auth.signIn(id);
      await complete();
    }));
  }
  document.querySelector<HTMLFormElement>("#email-form")!.hidden = auth.methods.includes("local");
  document.querySelector<HTMLFormElement>("#email-form")!.addEventListener("submit", (event) => {
    event.preventDefault();
    void run(async () => {
      if (emailLink) return finishEmail();
      const continuation = new URL(location.pathname, location.origin);
      continuation.search = new URLSearchParams({...challenge, provider:"email",mode:hint.mode}).toString();
      await auth.sendEmail(email.value, continuation.toString());
      sessionStorage.setItem("domios-signin-email", email.value);
      status.textContent = "Open the sign-in link sent to your email to continue.";
    });
  });
  providers.hidden = false;
  status.textContent = emailLink ? "Confirm your email address to continue." : "Choose a sign-in method.";
  const action = hintedAction(hint, auth.methods, emailLink);
  if (action === "finish-email" && email.value) await run(finishEmail);
  else if (action === "email") {
    for (const id of ["google", "facebook", "apple", "local"]) document.querySelector<HTMLButtonElement>(`#${id}`)!.hidden = true;
    email.focus();
    status.textContent = "Enter your email address to continue.";
  } else if (action === "google" || action === "facebook") {
    await run(async () => { await auth.signIn(action); await complete(); });
  }
}

void start().catch(() => { status.textContent = "This sign-in request is invalid. Start a new sign-in from Domios."; });
