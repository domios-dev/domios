import { providerName } from "../../src/adapters";
import SignIn from "./sign-in";
export const dynamic = "force-dynamic";
export default function SignInPage() { return <SignIn provider={providerName()} />; }
