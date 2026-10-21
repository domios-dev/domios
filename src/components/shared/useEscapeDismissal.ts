import { useEffect } from "react";

interface EscapeEvent {
  code: string;
  key: string;
  defaultPrevented: boolean;
  preventDefault: () => void;
  stopPropagation: () => void;
}

export function dismissOnEscape(event: EscapeEvent, onClose: () => void): boolean {
  if ((event.code !== "Escape" && event.key !== "Escape") || event.defaultPrevented) return false;
  event.preventDefault();
  event.stopPropagation();
  onClose();
  return true;
}

export function useEscapeDismissal(onClose: () => void) {
  useEffect(() => {
    const escape = (event: KeyboardEvent) => { dismissOnEscape(event, onClose); };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [onClose]);
}
