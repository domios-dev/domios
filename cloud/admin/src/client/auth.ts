export interface SignInClient { signIn(email: string, password: string): Promise<string> }
export async function signInClient(provider: string): Promise<SignInClient> {
  if (provider === "firebase") return (await import("./adapters/firebase")).client();
  if (provider === "local") return (await import("./adapters/local")).client();
  throw new Error("Interactive sign-in is unavailable");
}
