interface NotificationBadgeProps {
  count: number;
}

export function NotificationBadge({ count }: NotificationBadgeProps) {
  if (count <= 0) return null;

  return (
    <span
      className={`
        inline-flex items-center justify-center min-w-[16px] h-4 px-1
        rounded-full bg-[var(--ct-accent)] text-[var(--ct-bg)] text-[10px] font-bold leading-none
      `}
    >
      {count > 99 ? "+99" : `+${count}`}
    </span>
  );
}
