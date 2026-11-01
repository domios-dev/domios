import { useEffect, useRef, useState } from "react";
import { giftFailure, redeemGift, type GiftReceipt } from "./giftRedemptionModel";

export function useGiftRedemption(identity: string) {
  const [key, setKey] = useState("");
  const [pending, setPending] = useState(false);
  const [receipt, setReceipt] = useState<GiftReceipt | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useRef<AbortController | null>(null);
  const active = useRef(true);
  useEffect(() => {
    active.current = true;
    return () => { active.current = false; request.current?.abort(); };
  }, []);

  async function submit() {
    if (request.current || !key.trim()) return;
    const controller = new AbortController();
    request.current = controller;
    const entered = key.trim();
    setKey("");
    setPending(true);
    setReceipt(null);
    setError(null);
    const deadline = setTimeout(() => controller.abort(), 45_000);
    try {
      const result = await redeemGift(entered, identity, controller.signal);
      if (active.current) setReceipt(result);
    } catch (failure: unknown) {
      if (active.current) setError(giftFailure(failure instanceof Error ? failure.message : "unavailable"));
    } finally {
      clearTimeout(deadline);
      request.current = null;
      if (active.current) setPending(false);
    }
  }

  return { key, setKey, pending, receipt, error, submit };
}
