import type { ReactNode } from "react";
import { LogIn, Mail, UserPlus } from "lucide-react";
import type { LoginHint } from "./accountModel";

const googleIcon = new URL("../../assets/account/google.svg", import.meta.url).href;
const facebookIcon = new URL("../../assets/account/facebook-badge.svg", import.meta.url).href;

function BrandIcon({ source }: { source: string }) {
  return <img src={source} alt="" width={16} height={16} className="shrink-0" />;
}

export interface SignInOption {
  label: string;
  icon: ReactNode;
  hint: LoginHint;
}

export const ACCOUNT_ENTRY_OPTIONS: SignInOption[] = [
  { label: "Sign in", icon: <LogIn size={16} />, hint: { mode: "signin" } },
  { label: "Create account", icon: <UserPlus size={16} />, hint: { mode: "signup" } },
];

export const SIGN_IN_PROVIDER_OPTIONS: SignInOption[] = [
  { label: "Continue with Google", icon: <BrandIcon source={googleIcon} />, hint: { provider: "google", mode: "signin" } },
  { label: "Continue with Facebook", icon: <BrandIcon source={facebookIcon} />, hint: { provider: "facebook", mode: "signin" } },
  { label: "Continue with email", icon: <Mail size={16} />, hint: { provider: "email", mode: "signin" } },
];
