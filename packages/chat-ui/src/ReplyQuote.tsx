import { Quote, X } from "lucide-react";

/** A reply's quoted message: a neutral bar, the quote mark, who and when, and one line of what was said. */
export function ReplyQuote({ label, excerpt, onOpen, onRemove }: {
  label: string;
  excerpt: string;
  /** Sent form: the block is a button that shows the quoted message. */
  onOpen?: () => void;
  /** Draft form: the block carries a remove control. */
  onRemove?: () => void;
}) {
  const body = (
    <>
      <Quote size={12} aria-hidden className="ct-reply-quote__mark" />
      <span className="ct-reply-quote__text">
        <span className="ct-reply-quote__label">{label}</span>
        {excerpt && <span className="ct-reply-quote__excerpt">{excerpt}</span>}
      </span>
    </>
  );
  return (
    <div
      className="ct-reply-quote"
      data-testid="reply-quote"
      data-variant={onRemove ? "draft" : "sent"}
      role="group"
      aria-label={onRemove ? `Replying to ${label}` : `In reply to ${label}`}
    >
      {onOpen ? (
        <button type="button" className="ct-reply-quote__body" title="Show the quoted message" onClick={onOpen}>
          {body}
        </button>
      ) : (
        <div className="ct-reply-quote__body">{body}</div>
      )}
      {onRemove && (
        <button type="button" className="ct-reply-quote__remove" aria-label="Remove quote" title="Remove quote (Esc)" onClick={onRemove}>
          <X size={13} aria-hidden />
        </button>
      )}
    </div>
  );
}
