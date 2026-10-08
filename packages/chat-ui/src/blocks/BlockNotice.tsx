import React from "react";

type NoticeProps = React.HTMLAttributes<HTMLDivElement> & {
  name: string;
  label: string;
  source: string;
  icon: React.ReactNode;
  [dataAttribute: `data-${string}`]: string | boolean | undefined;
};

/** The muted one-line stand-in a block shows when it has nothing live to offer. */
export function BlockNotice({ name, label, source, icon, children, ...rest }: NoticeProps) {
  return (
    <div
      data-testid="chat-block"
      data-chat-block={name}
      role="note"
      aria-label={label}
      title={source}
      {...rest}
      className="my-2 flex w-fit max-w-[34rem] items-center gap-2 rounded-lg border px-2.5 py-1.5 text-xs"
      style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 22%, transparent)", color: "var(--ct-muted-fg)" }}
    >
      {icon}
      <span>{children}</span>
    </div>
  );
}
