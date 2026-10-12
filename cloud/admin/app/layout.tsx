import type { ReactNode } from "react";
import "./style.css";
export const metadata = { title: "Domios Admin", robots: { index: false, follow: false } };
export default function RootLayout({ children }: { children: ReactNode }) {
  return <html lang="en"><body><main>{children}</main></body></html>;
}
