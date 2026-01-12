import type { CandleData, DataSource, TimeFrame } from "./types";

type CacheKey = string;

export const candleCache = new Map<CacheKey, Map<number, CandleData>>();

const buildCacheKey = (
    source: DataSource,
    asset: string,
    quoteAsset: string,
    tf: TimeFrame
) =>
    `${source.exchange}:${source.market}:${asset}:${quoteAsset}:${tf}`.toUpperCase();

export function getTimeframeCache(
    source: DataSource,
    asset: string,
    quoteAsset: string,
    tf: TimeFrame
) {
    const cacheKey = buildCacheKey(source, asset, quoteAsset, tf);
    let tfCache = candleCache.get(cacheKey);
    if (!tfCache) {
        tfCache = new Map();
        candleCache.set(cacheKey, tfCache);
    }

    return tfCache;
}

export function clearCandleCache() {
    candleCache.clear();
}
