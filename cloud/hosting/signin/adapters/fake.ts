import type { AuthClient } from "../authClient";
import { parseChallenge } from "../../../shared/protocol";

export function fakeClient(): AuthClient {
  let signedIn = false;
  return {
    methods:["local"],isEmailLink:() => false,
    async signIn() { signedIn = true; },
    async sendEmail() {}, async finishEmail() { signedIn = true; },
    async createCode(challenge) {
      parseChallenge(challenge);
      if (!signedIn) throw new Error("Sign-in required");
      return "f".repeat(43);
    },
    async signOut() { signedIn = false; },
  };
}
