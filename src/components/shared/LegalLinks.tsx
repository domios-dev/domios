import { ExternalLink } from "lucide-react";
import type { ReactNode } from "react";
import { siteLinks } from "../../../shared/siteLinks";
import { nativeBridge } from "../../lib/nativeBridge";
import { AboutLink } from "../Settings/shared";
import type { DropdownMenuItem } from "./DropdownMenu";

export const LEGAL_DOCUMENTS = {
  terms: { title: "Terms of Service", short: "Terms", url: siteLinks.terms },
  privacy: { title: "Privacy Policy", short: "Privacy", url: siteLinks.privacy },
  license: { title: "License", short: "License", url: siteLinks.license },
  refunds: { title: "Refund policy", short: "14-day money-back guarantee", url: `${siteLinks.terms}#refunds` },
} as const;
export type LegalDocument = keyof typeof LEGAL_DOCUMENTS;

const openLegal = (url: string) => { void nativeBridge.openUrl(url); };

export function LegalLink({ document }: { document: LegalDocument }) {
  const { title, url } = LEGAL_DOCUMENTS[document];
  return <AboutLink href={url} label={title} Icon={ExternalLink} onOpen={openLegal} />;
}

/** A link that sits inside a sentence of fine print. */
export function InlineLegalLink({ document, children }: { document: LegalDocument; children?: ReactNode }) {
  const { short, url } = LEGAL_DOCUMENTS[document];
  return <a href={url} target="_blank" rel="noopener noreferrer"
    onClick={event => { event.preventDefault(); openLegal(url); }}
    className="text-[var(--ct-fg)] underline decoration-[color:var(--ct-border)] underline-offset-2 transition-colors hover:text-[var(--ct-accent)] hover:decoration-current focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">
    {children ?? short}
  </a>;
}

export function legalMenuItems(documents: readonly LegalDocument[]): DropdownMenuItem[] {
  return documents.map(document => ({ label: LEGAL_DOCUMENTS[document].title, onClick: () => openLegal(LEGAL_DOCUMENTS[document].url) }));
}

/** Disclosed before purchase: the refund window from the Terms and automatic renewal. */
export function SubscriptionTermsNotice() {
  return <p className="m-0 text-xs text-[var(--ct-muted-fg)]">
    <InlineLegalLink document="refunds" /> · Renews automatically, cancel anytime · <InlineLegalLink document="terms" /> · <InlineLegalLink document="privacy" />
  </p>;
}
