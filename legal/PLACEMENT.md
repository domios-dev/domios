# Legal Documents Placement Spec (for Iris)

> DRAFT pending owner and lawyer review. Spec only; this change builds no UI.

Canonical sources: `LICENSE`, `legal/ADDITIONAL-TERMS.md`, `legal/TERMS.md`, `legal/PRIVACY.md` in `rollacode/codeterm`. Each published document carries a version (for example `terms-2026-11`) and an effective date. Public pages on the website: `https://domios.dev/terms`, `https://domios.dev/privacy`, `https://domios.dev/license`.

## 1. Sign-in and account creation

- Where: the hosted sign-in page in the private `rollacode/codeterm-firebase` repo, `hosting/signin/index.html` and `hosting/signin/main.ts` (Sign in / Create account heading, Google, Facebook and email). The app's Account menu only opens this page in the system browser.
- Copy next to the sign-in / sign-up button, both modes, no checkbox: "By continuing, you agree to the [Terms] and [Privacy Policy]." Both links open the public URLs in a new tab.
- Recording: on successful sign-in or sign-up, silently store on the account record the terms version and privacy version shown, the server timestamp and the method (`signin_notice`). No UI.

## 2. Download

- Website download control in the private `rollacode/codeterm-web` repo: `src/download/DownloadActions.tsx` and `src/download/DownloadControl.tsx`. One line under the button: "Free to download. Use is subject to the [Domios License] and [Terms]."
- Installer: no click-through. The bundled `LICENSE` (`licenseFile` in `src-tauri/tauri.conf.json`) already appears in installers that show a license page. First launch: no extra modal.

## 3. Checkout and upgrade

- Paddle checkout shows Paddle's buyer terms; do not duplicate them.
- The in-app plans dialog and the hosted checkout (`codeterm-firebase/checkout/paddle.ts`) show above the purchase button: "Subscriptions renew automatically until cancelled. [Terms] · [Privacy]. Payments are processed by Paddle, our Merchant of Record."

## 4. App

- Settings > About: rows for Terms of Service, Privacy Policy and License; each opens the public URL in the system browser.
- Settings > Account and the signed-in Account menu: Terms and Privacy links next to Manage subscription.
- Settings > Privacy: Privacy Policy link next to What we collect.
- Terms changes: no in-app banner or re-acceptance. For a material change, email all registered users through Firebase ahead of the effective date with the new version link; continued use after the effective date is acceptance. Optionally store the notified version and send time per account.

## 5. Website

- Footer in `codeterm-web/src/App.tsx` (`site-footer` nav): add Terms, Privacy and License after GitHub; link targets belong in `src/siteLinks.ts`.
- New routes `/terms`, `/privacy`, `/license` rendering the canonical Markdown, with rewrites in `vercel.json`. The web repo owner implements this; this lane does not edit that repo.

## 6. Repository

- `legal/` holds the documents, `LICENSE` holds the full Domios License, and `NOTICE` and `README.md` link to them.
