import { getApps, initializeApp } from "firebase/app";
import { getAuth, setPersistence, inMemoryPersistence, signInWithEmailAndPassword, signOut } from "firebase/auth";
import type { SignInClient } from "../auth";

export function client(): SignInClient {
  const app = getApps()[0] ?? initializeApp({
    apiKey: process.env.NEXT_PUBLIC_AUTH_API_KEY,
    authDomain: process.env.NEXT_PUBLIC_AUTH_DOMAIN,
    projectId: process.env.NEXT_PUBLIC_AUTH_PROJECT_ID,
  });
  const auth = getAuth(app);
  return {
    async signIn(email, password) {
      await setPersistence(auth, inMemoryPersistence);
      try {
        const credential = await signInWithEmailAndPassword(auth, email, password);
        return await credential.user.getIdToken(true);
      } finally { await signOut(auth); }
    },
  };
}
