export const GALLERY_PATH_LIMIT = 32;
export const GALLERY_READ_CONCURRENCY = 2;

export function galleryPathPlan(paths: string[]) {
  return { paths: paths.slice(0, GALLERY_PATH_LIMIT), omitted: Math.max(0, paths.length - GALLERY_PATH_LIMIT) };
}

export async function loadGallerySlots<T>(paths: string[], load: (path: string) => Promise<T>, resolved: (index: number, result: { file: T } | { error: string }) => void, cancelled: () => boolean) {
  const boundedPaths = galleryPathPlan(paths).paths;
  let next = 0;
  const work = async () => {
    while (!cancelled() && next < boundedPaths.length) {
      const index = next++;
      let result: { file: T } | { error: string };
      try { result = { file: await load(boundedPaths[index]) }; }
      catch (error) { result = { error: error instanceof Error ? error.message : String(error) }; }
      if (!cancelled()) resolved(index, result);
    }
  };
  await Promise.all(Array.from({ length: Math.min(GALLERY_READ_CONCURRENCY, boundedPaths.length) }, work));
}
