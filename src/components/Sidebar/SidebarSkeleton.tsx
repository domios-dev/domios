import { Skeleton, SkeletonBlock } from "../shared/Skeleton";

const TAB_ROWS = [72, 56, 64, 48, 60];
const WORKSPACE_TILES = 8;
const RAIL_TILES = 6;

/** Placeholder rows with the sidebar's own rhythm while the workspace is restoring. */
export function SidebarSkeleton({ compact = false }: { compact?: boolean }) {
  if (compact) {
    return (
      <Skeleton data-testid="sidebar-skeleton" className="flex flex-col items-center gap-2 py-3">
        {Array.from({ length: RAIL_TILES }, (_, index) => <SkeletonBlock key={index} shape="tile" width={36} height={36} />)}
      </Skeleton>
    );
  }
  return (
    <Skeleton data-testid="sidebar-skeleton" className="flex flex-col gap-5 px-3 py-3">
      <div className="flex flex-col gap-3">
        <SkeletonBlock width={64} height={10} />
        {TAB_ROWS.map((width, index) => (
          <div key={index} className="flex items-center gap-2.5">
            <SkeletonBlock shape="circle" width={16} height={16} />
            <div className="flex min-w-0 flex-1 flex-col gap-1.5">
              <SkeletonBlock width={`${width}%`} height={10} />
              <SkeletonBlock width={`${width - 20}%`} height={8} faint />
            </div>
          </div>
        ))}
      </div>
      <div className="flex flex-col gap-3">
        <SkeletonBlock width={80} height={10} />
        <div className="grid grid-cols-5 gap-2">
          {Array.from({ length: WORKSPACE_TILES }, (_, index) => <SkeletonBlock key={index} shape="tile" className="aspect-square" />)}
        </div>
      </div>
    </Skeleton>
  );
}
