import { getReleaseInfo, releaseCache, fallbackCache } from '../../src/download/release';

export const config = { runtime: 'edge', regions: ['iad1'] };

export default async function handler(request: Request) {
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    return new Response(null, { status: 405, headers: { Allow: 'GET, HEAD' } });
  }
  const info = await getReleaseInfo();
  return new Response(request.method === 'HEAD' ? null : JSON.stringify(info), {
    headers: { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': info.version ? releaseCache : fallbackCache },
  });
}
