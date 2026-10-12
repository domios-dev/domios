import type { DesktopChallenge } from "../../shared/protocol";

export type SignInMethod = "google" | "facebook" | "apple" | "local";
export interface AuthClient {
  methods: SignInMethod[];
  isEmailLink(): boolean;
  signIn(method: SignInMethod): Promise<void>;
  sendEmail(email: string, continuation: string): Promise<void>;
  finishEmail(email: string): Promise<void>;
  createCode(challenge: DesktopChallenge): Promise<string>;
  signOut(): Promise<void>;
}
