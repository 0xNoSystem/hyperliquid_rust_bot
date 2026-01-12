import type { CandleData, DataSource, TimeFrame } from "./types";
import { DEFAULT_QUOTE_ASSET } from "./types";
import { getTimeframeCache } from "./candleCache";
import { fetchCandles } from "./dataSources";
import { TF_TO_MS } from "../types";

const abortError = () =>
    typeof DOMException === "undefined"
        ? new Error("Aborted")
        : new DOMException("Aborted", "AbortError");

function normalizeRange(
    startMs: number,
    endMs: number,
    candleIntervalMs: number
) {
    const clampedStart = Math.max(0, startMs);
    const normalizedStart = clampedStart - (clampedStart % candleIntervalMs);
    const normalizedEnd = Math.max(
        normalizedStart + candleIntervalMs,
        Math.ceil(endMs / candleIntervalMs) * candleIntervalMs
    );

    return { normalizedStart, normalizedEnd };
}

function collectCachedCandles(
    tfCache: Map<number, CandleData>,
    asset: string,
    normalizedStart: number,
    normalizedEnd: number,
    candleIntervalMs: number
) {
    const cached: CandleData[] = [];
    const missing: { start: number; end: number }[] = [];

    let gapStart: number | null = null;

    for (let ts = normalizedStart; ts < normalizedEnd; ts += candleIntervalMs) {
        const candle = tfCache.get(ts);

        if (candle && candle.asset === asset) {
            cached.push(candle);
            if (gapStart !== null) {
                missing.push({ start: gapStart, end: ts });
                gapStart = null;
            }
        } else if (gapStart === null) {
            gapStart = ts;
        }
    }

    if (gapStart !== null) {
        missing.push({ start: gapStart, end: normalizedEnd });
    }

    return { cached, missing };
}

function cacheToArray(tfCache: Map<number, CandleData>, asset: string) {
    return Array.from(tfCache.values())
        .filter((c) => c.asset === asset)
        .sort((a, b) => a.start - b.start);
}

export async function loadCandles(
    source: DataSource,
    tf: TimeFrame,
    startMs: number,
    endMs: number,
    asset: string,
    quoteAsset = DEFAULT_QUOTE_ASSET,
    setCached?: (c: CandleData[]) => void,
    signal?: AbortSignal
): Promise<CandleData[]> {
    if (!asset?.trim()) return [];

    const normalizedAsset = asset.trim().toUpperCase();
    const normalizedQuote =
        quoteAsset.trim().toUpperCase() || DEFAULT_QUOTE_ASSET;

    const candleIntervalMs = TF_TO_MS[tf];
    const prefetchBuffer = 200 * candleIntervalMs;

    let rangeStart = Math.max(0, startMs - prefetchBuffer);
    let rangeEnd = Math.min(Date.now(), endMs + prefetchBuffer);

    if (!rangeStart || !rangeEnd || rangeEnd <= rangeStart) {
        rangeEnd = Date.now();
        rangeStart = rangeEnd - 30 * 24 * 60 * 60 * 1000;
    }

    const { normalizedStart, normalizedEnd } = normalizeRange(
        rangeStart,
        rangeEnd,
        candleIntervalMs
    );
    const tfCache = getTimeframeCache(
        source,
        normalizedAsset,
        normalizedQuote,
        tf
    );
    const { cached, missing } = collectCachedCandles(
        tfCache,
        normalizedAsset,
        normalizedStart,
        normalizedEnd,
        candleIntervalMs
    );
    const fullCache = cacheToArray(tfCache, normalizedAsset);
    if (setCached) {
        setCached(fullCache);
    }

    if (missing.length === 0 && cached.length > 0) {
        return fullCache;
    }

    for (const segment of missing) {
        if (signal?.aborted) throw abortError();
        const data = await fetchCandles(
            source,
            normalizedAsset,
            normalizedQuote,
            segment.start,
            segment.end,
            tf,
            signal
        );

        for (const candle of data) {
            tfCache.set(candle.start, candle);
        }
    }

    const merged = cacheToArray(tfCache, normalizedAsset);

    return merged;
}
