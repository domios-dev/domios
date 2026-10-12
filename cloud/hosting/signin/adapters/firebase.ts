import { initializeApp, type FirebaseOptions } from "firebase/app";
import { initializeAppCheck, ReCaptchaEnterpriseProvider } from "firebase/app-check";
import {
  getAuth, setPersistence, inMemoryPersistence, GoogleAuthProvider, FacebookAuthProvider, OAuthProvider,
  signInWithPopup, sendSignInLinkToEmail, isSignInWithEmailLink, signInWithEmailLink, signOut,
} from "firebase/auth";
import { getFunctions, httpsCallable } from "firebase/functions";
import type { AuthClient, SignInMethod } from "../authClient";
import type { DesktopChallenge } from "../../../shared/protocol";

export interface FirebaseConfig { firebase: FirebaseOptions; appCheckSiteKey: string; appleEnabled: boolean }
export async function firebaseClient(config: FirebaseConfig): Promise<AuthClient> {
  if (!config.appCheckSiteKey || !config.firebase.projectId) throw new Error("Invalid configuration");
  const app = initializeApp(config.firebase);
  initializeAppCheck(app, {provider:new ReCaptchaEnterpriseProvider(config.appCheckSiteKey),isTokenAutoRefreshEnabled:true});
  const auth = getAuth(app);
  await setPersistence(auth,inMemoryPersistence);
  const methods: SignInMethod[] = config.appleEnabled === true ? ["google","facebook","apple"] : ["google","facebook"];
  const createCode = httpsCallable<DesktopChallenge,{code:string}>(getFunctions(app,"us-central1"),"createDesktopLoginCode");
  return {
    methods,
    isEmailLink:() => isSignInWithEmailLink(auth,location.href),
    async signIn(method) {
      if (!methods.includes(method)) throw new Error("Provider disabled");
      const provider = method === "google" ? new GoogleAuthProvider() : method === "facebook" ? new FacebookAuthProvider() : new OAuthProvider("apple.com");
      await signInWithPopup(auth,provider);
    },
    async sendEmail(email,url) { await sendSignInLinkToEmail(auth,email,{url,handleCodeInApp:true}); },
    async finishEmail(email) { await signInWithEmailLink(auth,email,location.href); },
    async createCode(challenge) { return (await createCode(challenge)).data.code; },
    signOut:() => signOut(auth),
  };
}
