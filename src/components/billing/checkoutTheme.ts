export type CheckoutTheme = "light" | "dark";

/** The app's resolved appearance (App sets `data-ct-theme`), never "system", so checkout opens to match. */
export function checkoutTheme(root: { dataset: DOMStringMap } = document.documentElement): CheckoutTheme {
  return root.dataset.ctTheme === "light" ? "light" : "dark";
}
