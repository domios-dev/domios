import { killDevConsole } from "./dev-console";

export default function globalTeardown(): void {
  killDevConsole();
}
