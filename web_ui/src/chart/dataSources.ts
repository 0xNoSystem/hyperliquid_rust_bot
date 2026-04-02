import type {
    CandleData,
    DataSource,
    ExchangeId,
    MarketType,
    TimeFrame,
} from "./types";
import { TF_TO_MS, fromTimeFrame } from "../types";

type IntervalPlan = {
    interval: string;
    baseTimeframe: TimeFrame;
    groupSize: number;
};

type IntervalMap = Partial<Record<TimeFrame, string>>;

type ExchangeIntervalMap = Record<MarketType, IntervalMap>;

type FetchArgs = {
    source: DataSource;
    asset: string;
    quoteAsset: string;
    startTime: number;
    endTime: number;
    interval: string;
    intervalLabel: string;
    baseIntervalMs: number;
    signal?: AbortSignal;
};

type BinanceKline = [
    number,
    string,
    string,
    string,
    string,
    string,
    number,
    string,
    number,
    string,
    string,
];

const BINANCE_INTERVALS: Record<TimeFrame, string> = {
    min1: "1m",
    min3: "3m",
    min5: "5m",
    min15: "15m",
    min30: "30m",
    hour1: "1h",
    hour2: "2h",
    hour4: "4h",
    hour12: "12h",
    day1: "1d",
    day3: "3d",
    week: "1w",
    month: "1M",
};

const BYBIT_INTERVALS: IntervalMap = {
    min1: "1",
    min3: "3",
    min5: "5",
    min15: "15",
    min30: "30",
    hour1: "60",
    hour2: "120",
    hour4: "240",
    hour12: "720",
    day1: "D",
    week: "W",
    month: "M",
};

const HTX_SPOT_INTERVALS: IntervalMap = {
    min1: "1min",
    min5: "5min",
    min15: "15min",
    min30: "30min",
    hour1: "60min",
    hour4: "4hour",
    day1: "1day",
    week: "1week",
    month: "1mon",
};

const HTX_FUTURES_INTERVALS: IntervalMap = {
    min1: "1min",
    min5: "5min",
    min15: "15min",
    min30: "30min",
    hour1: "60min",
    hour4: "4hour",
    day1: "1day",
    week: "1week",
    month: "1mon",
};

const EXCHANGE_INTERVALS: Record<ExchangeId, ExchangeIntervalMap> = {
    binance: { spot: BINANCE_INTERVALS, futures: BINANCE_INTERVALS },
    bybit: { spot: BYBIT_INTERVALS, futures: BYBIT_INTERVALS },
    htx: { spot: HTX_SPOT_INTERVALS, futures: HTX_FUTURES_INTERVALS },
};

const abortError = () =>
    typeof DOMException === "undefined"
        ? new Error("Aborted")
        : new DOMException("Aborted", "AbortError");

const throwIfAborted = (signal?: AbortSignal) => {
    if (signal?.aborted) throw abortError();
};

const buildQuery = (params: Record<string, string | number | undefined>) => {
    const search = new URLSearchParams();
    for (const [key, value] of Object.entries(params)) {
        if (value === undefined || value === "") continue;
        search.set(key, String(value));
    }
    const query = search.toString();
    return query ? `?${query}` : "";
};

const normalizeSymbolPart = (value: string) => value.trim().toUpperCase();

const resolveSymbol = (
    asset: string,
    quoteAsset: string,
    separator: string,
    suffix = "",
    lowercase = false
) => {
    const base = normalizeSymbolPart(asset);
    const quote = normalizeSymbolPart(quoteAsset);
    const hasSeparator = base.includes("-") || base.includes("_");
    const hasQuote = base.includes(quote);
    const symbol =
        hasSeparator || hasQuote
            ? base
            : `${base}${separator}${quote}${suffix}`;
    return lowercase ? symbol.toLowerCase() : symbol;
};

const dedupeCandles = (candles: CandleData[]) => {
    const map = new Map<number, CandleData>();
    for (const candle of candles) {
        map.set(candle.start, candle);
    }
    return Array.from(map.values());
};

const sortCandles = (candles: CandleData[]) =>
    candles.sort((a, b) => a.start - b.start);

const aggregateCandles = (
    candles: CandleData[],
    targetMs: number,
    asset: string,
    intervalLabel: string
) => {
    if (candles.length === 0) return [];
    const sorted = sortCandles([...candles]);
    const out: CandleData[] = [];
    let bucketStart: number | null = null;
    let bucket: CandleData | null = null;

    for (const candle of sorted) {
        const start = Math.floor(candle.start / targetMs) * targetMs;
        if (bucketStart === null || start !== bucketStart) {
            if (bucket) out.push(bucket);
            bucketStart = start;
            bucket = {
                start,
                end: start + targetMs,
                open: candle.open,
                high: candle.high,
                low: candle.low,
                close: candle.close,
                volume: candle.volume,
                trades: candle.trades,
                asset,
                interval: intervalLabel,
            };
        } else if (bucket) {
            bucket.high = Math.max(bucket.high, candle.high);
            bucket.low = Math.min(bucket.low, candle.low);
            bucket.close = candle.close;
            bucket.volume += candle.volume;
            bucket.trades += candle.trades;
        }
    }

    if (bucket) out.push(bucket);
    return out;
};

const getIntervalPlan = (
    source: DataSource,
    tf: TimeFrame
): IntervalPlan | null => {
    const marketIntervals =
        EXCHANGE_INTERVALS[source.exchange]?.[source.market];
    if (!marketIntervals) return null;

    const direct = marketIntervals[tf];
    if (direct) {
        return { interval: direct, baseTimeframe: tf, groupSize: 1 };
    }

    const targetMs = TF_TO_MS[tf];
    let best: TimeFrame | null = null;
    for (const [candidate, interval] of Object.entries(marketIntervals) as [
        TimeFrame,
        string | undefined,
    ][]) {
        if (!interval) continue;
        const baseMs = TF_TO_MS[candidate];
        if (baseMs > targetMs) continue;
        if (targetMs % baseMs !== 0) continue;
        if (!best || baseMs > TF_TO_MS[best]) {
            best = candidate;
        }
    }

    if (!best || !marketIntervals[best]) {
        return null;
    }

    const baseMs = TF_TO_MS[best];
    return {
        interval: marketIntervals[best] as string,
        baseTimeframe: best,
        groupSize: targetMs / baseMs,
    };
};

const resolveIntervalPlan = (
    source: DataSource,
    tf: TimeFrame
): IntervalPlan => {
    const plan = getIntervalPlan(source, tf);
    if (plan) return plan;

    const marketIntervals =
        EXCHANGE_INTERVALS[source.exchange]?.[source.market];
    if (!marketIntervals) {
        throw new Error(`Source not supported: ${source.exchange}`);
    }

    throw new Error(
        `Timeframe ${tf} not supported for ${source.exchange} ${source.market}`
    );
};

export const isTimeframeSupported = (source: DataSource, tf: TimeFrame) =>
    getIntervalPlan(source, tf) !== null;

const clampToRange = (
    candles: CandleData[],
    startTime: number,
    endTime: number
) => candles.filter((c) => c.end > startTime && c.start < endTime);

export async function fetchCandles(
    source: DataSource,
    asset: string,
    quoteAsset: string,
    startTime: number,
    endTime: number,
    tf: TimeFrame,
    signal?: AbortSignal
): Promise<CandleData[]> {
    const normalizedAsset = normalizeSymbolPart(asset);
    const normalizedQuote = normalizeSymbolPart(quoteAsset || "USDT");
    const plan = resolveIntervalPlan(source, tf);
    const baseIntervalMs = TF_TO_MS[plan.baseTimeframe];
    const intervalLabel = fromTimeFrame(plan.baseTimeframe);

    const raw = await fetchCandlesForSource({
        source,
        asset: normalizedAsset,
        quoteAsset: normalizedQuote,
        startTime,
        endTime,
        interval: plan.interval,
        intervalLabel,
        baseIntervalMs,
        signal,
    });

    const filtered = sortCandles(
        dedupeCandles(clampToRange(raw, startTime, endTime))
    );

    if (plan.groupSize === 1) {
        const finalLabel = fromTimeFrame(tf);
        return filtered.map((c) => ({ ...c, interval: finalLabel }));
    }

    return aggregateCandles(
        filtered,
        TF_TO_MS[tf],
        normalizedAsset,
        fromTimeFrame(tf)
    );
}

async function fetchCandlesForSource(args: FetchArgs): Promise<CandleData[]> {
    switch (args.source.exchange) {
        case "binance":
            return fetchBinanceCandles(args);
        case "bybit":
            return fetchBybitCandles(args);
        case "htx":
            return fetchHtxCandles(args);
        default:
            throw new Error(`Source not supported: ${args.source.exchange}`);
    }
}

async function fetchBinanceCandles({
    source,
    asset,
    quoteAsset,
    startTime,
    endTime,
    interval,
    intervalLabel,
    baseIntervalMs,
    signal,
}: FetchArgs): Promise<CandleData[]> {
    const symbol = resolveSymbol(asset, quoteAsset, "");
    const baseUrl =
        source.market === "spot"
            ? "https://api.binance.com/api/v3/klines"
            : "https://fapi.binance.com/fapi/v1/klines";
    const limit = source.market === "spot" ? 1000 : 1500;
    const all: CandleData[] = [];
    let cursor = startTime;

    while (cursor < endTime) {
        throwIfAborted(signal);
        const url =
            baseUrl +
            buildQuery({
                symbol,
                interval,
                startTime: cursor,
                endTime,
                limit,
            });

        const res = await fetch(url, { signal });
        if (!res.ok) {
            throw new Error(`Binance ${source.market} error ${res.status}`);
        }
        const data = (await res.json()) as BinanceKline[];
        if (!Array.isArray(data) || data.length === 0) break;

        for (const k of data) {
            const start = Number(k[0]);
            const end = Number(k[6]) || start + baseIntervalMs;
            all.push({
                start,
                open: Number(k[1]),
                high: Number(k[2]),
                low: Number(k[3]),
                close: Number(k[4]),
                volume: Number(k[5]) || 0,
                end,
                trades: Number(k[8]) || 0,
                asset,
                interval: intervalLabel,
            });
        }

        const lastStart = Number(data[data.length - 1][0]);
        if (!Number.isFinite(lastStart) || lastStart <= cursor) break;
        cursor = lastStart + 1;
        if (data.length < limit) break;
    }

    return all;
}

async function fetchBybitCandles({
    source,
    asset,
    quoteAsset,
    startTime,
    endTime,
    interval,
    intervalLabel,
    baseIntervalMs,
    signal,
}: FetchArgs): Promise<CandleData[]> {
    const symbol = resolveSymbol(asset, quoteAsset, "");
    const baseUrl = "https://api.bybit.com/v5/market/kline";
    const category = source.market === "spot" ? "spot" : "linear";
    const limit = 1000;
    const all: CandleData[] = [];
    let cursor = startTime;

    while (cursor < endTime) {
        throwIfAborted(signal);
        const url =
            baseUrl +
            buildQuery({
                category,
                symbol,
                interval,
                start: cursor,
                end: endTime,
                limit,
            });
        const res = await fetch(url, { signal });
        if (!res.ok) {
            throw new Error(`Bybit error ${res.status}`);
        }
        const json = (await res.json()) as {
            retCode?: number;
            retMsg?: string;
            result?: { list?: string[][] };
        };
        if (json.retCode && json.retCode !== 0) {
            throw new Error(json.retMsg || `Bybit error ${json.retCode}`);
        }
        const list = json.result?.list ?? [];
        if (!Array.isArray(list) || list.length === 0) break;

        let maxStart = cursor;
        for (const k of list) {
            const start = Number(k[0]);
            const end = start + baseIntervalMs;
            if (start > maxStart) maxStart = start;
            all.push({
                start,
                open: Number(k[1]),
                high: Number(k[2]),
                low: Number(k[3]),
                close: Number(k[4]),
                volume: Number(k[5]) || 0,
                end,
                trades: 0,
                asset,
                interval: intervalLabel,
            });
        }

        if (!Number.isFinite(maxStart) || maxStart <= cursor) break;
        cursor = maxStart + baseIntervalMs;
        if (list.length < limit) break;
    }

    return all;
}

async function fetchHtxCandles({
    source,
    asset,
    quoteAsset,
    startTime,
    endTime,
    interval,
    intervalLabel,
    baseIntervalMs,
    signal,
}: FetchArgs): Promise<CandleData[]> {
    const isSpot = source.market === "spot";
    const baseUrl = isSpot
        ? "https://api.huobi.pro/market/history/kline"
        : "https://api.hbdm.com/linear-swap-ex/market/history/kline";
    const symbol = isSpot
        ? resolveSymbol(asset, quoteAsset, "", "", true)
        : resolveSymbol(asset, quoteAsset, "-");
    const size = Math.min(
        2000,
        Math.max(1, Math.ceil((endTime - startTime) / baseIntervalMs) + 10)
    );
    const params = isSpot
        ? { symbol, period: interval, size }
        : { contract_code: symbol, period: interval, size };

    throwIfAborted(signal);
    const res = await fetch(baseUrl + buildQuery(params), { signal });
    if (!res.ok) {
        throw new Error(`HTX error ${res.status}`);
    }
    const json = (await res.json()) as {
        data?: Array<{
            id: number;
            open: number;
            high: number;
            low: number;
            close: number;
            vol?: number;
            amount?: number;
            count?: number;
        }>;
    };
    const data = json.data ?? [];
    return data.map((k) => {
        const start = Number(k.id) * 1000;
        return {
            start,
            open: Number(k.open),
            high: Number(k.high),
            low: Number(k.low),
            close: Number(k.close),
            volume: Number(k.vol ?? k.amount ?? 0),
            end: start + baseIntervalMs,
            trades: Number(k.count ?? 0),
            asset,
            interval: intervalLabel,
        };
    });
}

