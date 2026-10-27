import { useId } from "react";
import { Button } from "../shared/Button";
import { giftDurationLabel } from "./giftRedemption";
import { tierName } from "./desktopBilling";
import { useGiftRedemption } from "./useGiftRedemption";

export function GiftRedemption({ identity, verifiedPlan }: { identity: string; verifiedPlan: string | null }) {
  const gift = useGiftRedemption(identity);
  const inputId = useId();
  return <form aria-label="Redeem a gift key" className="flex w-full flex-col gap-2"
    onSubmit={event => { event.preventDefault(); void gift.submit(); }}>
    <label htmlFor={inputId}>Gift key</label>
    <input id={inputId} type="password" autoComplete="off" spellCheck={false} maxLength={46}
      value={gift.key} disabled={gift.pending} onChange={event => gift.setKey(event.target.value)} />
    <Button type="submit" size="sm" disabled={gift.pending || !gift.key.trim()} aria-busy={gift.pending}>
      {gift.pending ? "Redeeming…" : "Redeem gift key"}
    </Button>
    {gift.error && <p role="alert">{gift.error}</p>}
    {gift.receipt && <div role="status">
      <p>Gift redeemed: {tierName(gift.receipt.plan)}, {giftDurationLabel[gift.receipt.duration]}.
        {gift.receipt.expires_at === null ? " No expiry." : ` Ends ${new Date(gift.receipt.expires_at * 1000).toLocaleDateString()}.`}</p>
      <p>Current access plan: {verifiedPlan ? tierName(verifiedPlan) : "Checking access"}.</p>
    </div>}
  </form>;
}
