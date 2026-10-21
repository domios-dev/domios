import type { ReactNode } from "react";
import { Skeleton, SkeletonBlock } from "../../shared/Skeleton";
import { BoardPanel } from "./BoardPanel";

export function BoardCardSkeleton({ title, testId }: { title: string; testId: string }) {
  return <BoardPanel title={title} testId={testId}>
    <div role="status" aria-label={`Loading ${title}`} aria-busy="true">
      <Skeleton className="flex flex-col gap-3 p-4">
        <SkeletonBlock width="55%" height={28} />
        <SkeletonBlock width="90%" height={12} />
        <SkeletonBlock width="75%" height={12} />
      </Skeleton>
    </div>
  </BoardPanel>;
}

export function BoardCardUnavailable({ title, testId }: { title: string; testId: string }) {
  return <BoardPanel title={title} testId={testId}><p role="status" className="mc-board-note">Reading unavailable.</p></BoardPanel>;
}

export function BoardCardReading({ title, testId, ready, error, children }: {
  title: string;
  testId: string;
  ready: boolean;
  error?: string | null;
  children: ReactNode;
}) {
  if (ready) return <>{children}</>;
  return error ? <BoardCardUnavailable title={title} testId={testId} /> : <BoardCardSkeleton title={title} testId={testId} />;
}
