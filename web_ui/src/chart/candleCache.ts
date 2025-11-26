import type { TimeFrame } from "../types";
import type { CandleData } from "./utils";

export const candleCache = new Map<TimeFrame, Map<number, CandleData>>();

const cacheOwners = new Map<TimeFrame, string>();

export function getTimeframeCache(tf: TimeFrame, asset: string) {
    const owner = cacheOwners.get(tf);

    if (owner && owner !== asset) {
        candleCache.set(tf, new Map());
    }

    cacheOwners.set(tf, asset);

    let tfCache = candleCache.get(tf);
    if (!tfCache) {
        tfCache = new Map();
        candleCache.set(tf, tfCache);
    }

    return tfCache;
}

export function clearCandleCache() {
    candleCache.clear();
    cacheOwners.clear();
}
