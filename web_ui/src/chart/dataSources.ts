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

const OKX_INTERVALS: IntervalMap = {
    min1: "1m",
    min3: "3m",
    min5: "5m",
    min15: "15m",
    min30: "30m",
    hour1: "1H",
    hour2: "2H",
    hour4: "4H",
    hour12: "12H",
    day1: "1D",
    day3: "3D",
    week: "1W",
    month: "1M",
};

const KUCOIN_SPOT_INTERVALS: IntervalMap = {
    min1: "1min",
    min3: "3min",
    min5: "5min",
    min15: "15min",
    min30: "30min",
    hour1: "1hour",
    hour2: "2hour",
    hour4: "4hour",
    hour12: "12hour",
    day1: "1day",
    week: "1week",
    month: "1month",
};

const KUCOIN_FUTURES_INTERVALS: IntervalMap = {
    min1: "60",
    min3: "180",
    min5: "300",
    min15: "900",
    min30: "1800",
    hour1: "3600",
    hour2: "7200",
    hour4: "14400",
    hour12: "43200",
    day1: "86400",
    day3: "259200",
    week: "604800",
    month: "2592000",
};

const GATEIO_INTERVALS: IntervalMap = {
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

const COINBASE_SPOT_INTERVALS: IntervalMap = {
    min1: "60",
    min5: "300",
    min15: "900",
    hour1: "3600",
    day1: "86400",
};

const COINBASE_FUTURES_INTERVALS: IntervalMap = {};

const KRAKEN_SPOT_INTERVALS: IntervalMap = {
    min1: "1",
    min5: "5",
    min15: "15",
    min30: "30",
    hour1: "60",
    hour4: "240",
    day1: "1440",
    week: "10080",
};

const KRAKEN_FUTURES_INTERVALS: IntervalMap = {
    min1: "1",
    min5: "5",
    min15: "15",
    min30: "30",
    hour1: "60",
    hour4: "240",
    day1: "1440",
    week: "10080",
};

const BITGET_SPOT_INTERVALS: IntervalMap = {
    min1: "1min",
    min5: "5min",
    min15: "15min",
    min30: "30min",
    hour1: "1h",
    hour4: "4h",
    day1: "1day",
    week: "1week",
    month: "1month",
};

const BITGET_FUTURES_INTERVALS: IntervalMap = {
    min1: "60",
    min3: "180",
    min5: "300",
    min15: "900",
    min30: "1800",
    hour1: "3600",
    hour2: "7200",
    hour4: "14400",
    hour12: "43200",
    day1: "86400",
    day3: "259200",
    week: "604800",
    month: "2592000",
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

const MEXC_SPOT_INTERVALS: IntervalMap = {
    min1: "1m",
    min5: "5m",
    min15: "15m",
    min30: "30m",
    hour1: "60m",
    hour4: "4h",
    day1: "1d",
    week: "1w",
    month: "1M",
};

const MEXC_FUTURES_INTERVALS: IntervalMap = {
    min1: "Min1",
    min5: "Min5",
    min15: "Min15",
    min30: "Min30",
    hour1: "Min60",
    hour4: "Hour4",
    day1: "Day1",
    week: "Week1",
    month: "Month1",
};

const EXCHANGE_INTERVALS: Record<ExchangeId, ExchangeIntervalMap> = {
    binance: { spot: BINANCE_INTERVALS, futures: BINANCE_INTERVALS },
    bybit: { spot: BYBIT_INTERVALS, futures: BYBIT_INTERVALS },
    okx: { spot: OKX_INTERVALS, futures: OKX_INTERVALS },
    kucoin: { spot: KUCOIN_SPOT_INTERVALS, futures: KUCOIN_FUTURES_INTERVALS },
    gateio: { spot: GATEIO_INTERVALS, futures: GATEIO_INTERVALS },
    coinbase: {
        spot: COINBASE_SPOT_INTERVALS,
        futures: COINBASE_FUTURES_INTERVALS,
    },
    kraken: { spot: KRAKEN_SPOT_INTERVALS, futures: KRAKEN_FUTURES_INTERVALS },
    bitget: { spot: BITGET_SPOT_INTERVALS, futures: BITGET_FUTURES_INTERVALS },
    htx: { spot: HTX_SPOT_INTERVALS, futures: HTX_FUTURES_INTERVALS },
    mexc: { spot: MEXC_SPOT_INTERVALS, futures: MEXC_FUTURES_INTERVALS },
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

const normalizeKrakenAsset = (asset: string) => {
    const upper = normalizeSymbolPart(asset);
    return upper === "BTC" ? "XBT" : upper;
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
        case "okx":
            return fetchOkxCandles(args);
        case "kucoin":
            return fetchKucoinCandles(args);
        case "gateio":
            return fetchGateioCandles(args);
        case "coinbase":
            return fetchCoinbaseCandles(args);
        case "kraken":
            return fetchKrakenCandles(args);
        case "bitget":
            return fetchBitgetCandles(args);
        case "htx":
            return fetchHtxCandles(args);
        case "mexc":
            return fetchMexcCandles(args);
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

async function fetchOkxCandles({
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
    const instId =
        source.market === "spot"
            ? resolveSymbol(asset, quoteAsset, "-")
            : `${resolveSymbol(asset, quoteAsset, "-")}-SWAP`;
    const baseUrl = "https://www.okx.com/api/v5/market/candles";
    const limit = 100;
    const all: CandleData[] = [];
    let before = endTime;

    while (before > startTime) {
        throwIfAborted(signal);
        const url =
            baseUrl +
            buildQuery({
                instId,
                bar: interval,
                before,
                limit,
            });
        const res = await fetch(url, { signal });
        if (!res.ok) {
            throw new Error(`OKX error ${res.status}`);
        }
        const json = (await res.json()) as {
            code?: string;
            msg?: string;
            data?: string[][];
        };
        if (json.code && json.code !== "0") {
            throw new Error(json.msg || `OKX error ${json.code}`);
        }
        const data = json.data ?? [];
        if (!Array.isArray(data) || data.length === 0) break;

        let minStart = before;
        for (const k of data) {
            const start = Number(k[0]);
            if (start < minStart) minStart = start;
            all.push({
                start,
                open: Number(k[1]),
                high: Number(k[2]),
                low: Number(k[3]),
                close: Number(k[4]),
                volume: Number(k[5]) || 0,
                end: start + baseIntervalMs,
                trades: 0,
                asset,
                interval: intervalLabel,
            });
        }

        if (!Number.isFinite(minStart) || minStart >= before) break;
        before = minStart - 1;
        if (data.length < limit) break;
    }

    return all;
}

async function fetchKucoinCandles({
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
        ? "https://api.kucoin.com/api/v1/market/candles"
        : "https://api-futures.kucoin.com/api/v1/kline/query";
    const symbol = isSpot
        ? resolveSymbol(asset, quoteAsset, "-")
        : resolveSymbol(asset, quoteAsset, "", "M");
    const startSec = Math.floor(startTime / 1000);
    let cursorEnd = Math.floor(endTime / 1000);
    const all: CandleData[] = [];

    while (cursorEnd > startSec) {
        throwIfAborted(signal);
        const params = isSpot
            ? {
                  symbol,
                  type: interval,
                  startAt: startSec,
                  endAt: cursorEnd,
              }
            : {
                  symbol,
                  granularity: interval,
                  from: startSec,
                  to: cursorEnd,
              };
        const url = baseUrl + buildQuery(params);
        const res = await fetch(url, { signal });
        if (!res.ok) {
            throw new Error(`KuCoin error ${res.status}`);
        }
        const json = (await res.json()) as {
            code?: string;
            data?: string[][] | { data?: string[][] };
        };
        if (json.code && json.code !== "200000") {
            throw new Error(json.code);
        }
        const payload = Array.isArray(json.data)
            ? json.data
            : json.data?.data || [];
        if (!Array.isArray(payload) || payload.length === 0) break;

        let minStart = cursorEnd;
        for (const k of payload) {
            const start = Number(k[0]) * 1000;
            const open = Number(k[1]);
            const close = Number(k[2]);
            const high = Number(k[3]);
            const low = Number(k[4]);
            const volume = Number(k[5]) || 0;
            if (start < minStart * 1000) minStart = Math.floor(start / 1000);
            all.push({
                start,
                open,
                high,
                low,
                close,
                volume,
                end: start + baseIntervalMs,
                trades: 0,
                asset,
                interval: intervalLabel,
            });
        }

        if (!Number.isFinite(minStart) || minStart >= cursorEnd) break;
        cursorEnd = minStart - 1;
        if (cursorEnd <= startSec) break;
    }

    return all;
}

async function fetchGateioCandles({
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
        ? "https://api.gateio.ws/api/v4/spot/candlesticks"
        : "https://api.gateio.ws/api/v4/futures/usdt/candlesticks";
    const symbol = resolveSymbol(asset, quoteAsset, "_");
    const limit = 1000;
    const startSec = Math.floor(startTime / 1000);
    let cursorEnd = Math.floor(endTime / 1000);
    const all: CandleData[] = [];

    while (cursorEnd > startSec) {
        throwIfAborted(signal);
        const params = isSpot
            ? {
                  currency_pair: symbol,
                  interval,
                  from: startSec,
                  to: cursorEnd,
                  limit,
              }
            : {
                  contract: symbol,
                  interval,
                  from: startSec,
                  to: cursorEnd,
                  limit,
              };
        const url = baseUrl + buildQuery(params);
        const res = await fetch(url, { signal });
        if (!res.ok) {
            throw new Error(`Gate.io error ${res.status}`);
        }
        const data = (await res.json()) as string[][];
        if (!Array.isArray(data) || data.length === 0) break;

        let minStart = cursorEnd;
        for (const k of data) {
            const start = Number(k[0]) * 1000;
            const volume = Number(k[1]) || 0;
            const close = Number(k[2]);
            const high = Number(k[3]);
            const low = Number(k[4]);
            const open = Number(k[5]);
            if (start < minStart * 1000) minStart = Math.floor(start / 1000);
            all.push({
                start,
                open,
                high,
                low,
                close,
                volume,
                end: start + baseIntervalMs,
                trades: 0,
                asset,
                interval: intervalLabel,
            });
        }

        if (!Number.isFinite(minStart) || minStart >= cursorEnd) break;
        cursorEnd = minStart - 1;
        if (data.length < limit) break;
    }

    return all;
}

async function fetchCoinbaseCandles({
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
    if (source.market !== "spot") {
        throw new Error("Coinbase futures not supported yet");
    }
    const productId = resolveSymbol(asset, quoteAsset, "-");
    const baseUrl = `https://api.coinbase.com/api/v3/brokerage/products/${productId}/candles`;
    const startIso = new Date(startTime).toISOString();
    const endIso = new Date(endTime).toISOString();
    const url =
        baseUrl +
        buildQuery({ granularity: interval, start: startIso, end: endIso });

    throwIfAborted(signal);
    const res = await fetch(url, { signal });
    if (!res.ok) {
        throw new Error(`Coinbase error ${res.status}`);
    }
    const json = (await res.json()) as {
        candles?: Array<
            | [number, number, number, number, number, number]
            | {
                  start: string | number;
                  open: string | number;
                  high: string | number;
                  low: string | number;
                  close: string | number;
                  volume: string | number;
              }
        >;
    };

    const candles = json.candles ?? [];
    const out: CandleData[] = [];
    for (const candle of candles) {
        if (Array.isArray(candle)) {
            const start = Number(candle[0]) * 1000;
            out.push({
                start,
                open: Number(candle[3]),
                high: Number(candle[2]),
                low: Number(candle[1]),
                close: Number(candle[4]),
                volume: Number(candle[5]) || 0,
                end: start + baseIntervalMs,
                trades: 0,
                asset,
                interval: intervalLabel,
            });
        } else {
            const startValue =
                typeof candle.start === "string"
                    ? Date.parse(candle.start)
                    : Number(candle.start) * 1000;
            out.push({
                start: startValue,
                open: Number(candle.open),
                high: Number(candle.high),
                low: Number(candle.low),
                close: Number(candle.close),
                volume: Number(candle.volume) || 0,
                end: startValue + baseIntervalMs,
                trades: 0,
                asset,
                interval: intervalLabel,
            });
        }
    }

    return out;
}

async function fetchKrakenCandles({
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
    if (source.market === "futures") {
        return fetchKrakenFuturesCandles({
            source,
            asset,
            quoteAsset,
            startTime,
            endTime,
            interval,
            intervalLabel,
            baseIntervalMs,
            signal,
        });
    }

    const base = normalizeKrakenAsset(asset);
    const quote = normalizeSymbolPart(quoteAsset);
    const pair = `${base}${quote}`;
    const baseUrl = "https://api.kraken.com/0/public/OHLC";
    const all: CandleData[] = [];
    let since = Math.floor(startTime / 1000);
    const endSec = Math.floor(endTime / 1000);

    while (since < endSec) {
        throwIfAborted(signal);
        const url = baseUrl + buildQuery({ pair, interval, since });
        const res = await fetch(url, { signal });
        if (!res.ok) {
            throw new Error(`Kraken error ${res.status}`);
        }
        const json = (await res.json()) as {
            error?: string[];
            result?: Record<string, unknown> & { last?: string };
        };
        if (json.error && json.error.length > 0) {
            throw new Error(json.error.join(", "));
        }
        const result = json.result ?? {};
        const key = Object.keys(result).find((k) => k !== "last");
        const list = (key ? (result[key] as string[][]) : []) ?? [];
        if (!Array.isArray(list) || list.length === 0) break;

        for (const k of list) {
            const start = Number(k[0]) * 1000;
            all.push({
                start,
                open: Number(k[1]),
                high: Number(k[2]),
                low: Number(k[3]),
                close: Number(k[4]),
                volume: Number(k[6]) || 0,
                end: start + baseIntervalMs,
                trades: Number(k[7]) || 0,
                asset,
                interval: intervalLabel,
            });
        }

        const last = Number((result as { last?: string }).last || "0");
        if (!Number.isFinite(last) || last <= since) break;
        since = last;
        if (since >= endSec) break;
    }

    return all;
}

async function fetchKrakenFuturesCandles({
    asset,
    quoteAsset,
    startTime,
    endTime,
    interval,
    intervalLabel,
    baseIntervalMs,
    signal,
}: FetchArgs): Promise<CandleData[]> {
    const base = normalizeKrakenAsset(asset);
    const quote = normalizeSymbolPart(quoteAsset);
    const symbol = `PI_${base}${quote}`;
    const baseUrl =
        "https://futures.kraken.com/derivatives/api/v3/charts/v1/candles";
    const startSec = Math.floor(startTime / 1000);
    const endSec = Math.floor(endTime / 1000);
    const url =
        baseUrl +
        buildQuery({
            symbol,
            resolution: interval,
            from: startSec,
            to: endSec,
        });

    throwIfAborted(signal);
    const res = await fetch(url, { signal });
    if (!res.ok) {
        throw new Error(`Kraken futures error ${res.status}`);
    }
    const json = (await res.json()) as {
        result?: {
            candles?: Array<{
                time: number;
                open: number;
                high: number;
                low: number;
                close: number;
                volume: number;
            }>;
        };
    };
    const candles = json.result?.candles ?? [];
    return candles.map((c) => ({
        start: c.time * 1000,
        open: Number(c.open),
        high: Number(c.high),
        low: Number(c.low),
        close: Number(c.close),
        volume: Number(c.volume) || 0,
        end: c.time * 1000 + baseIntervalMs,
        trades: 0,
        asset,
        interval: intervalLabel,
    }));
}

async function fetchBitgetCandles({
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
        ? "https://api.bitget.com/api/spot/v1/market/candles"
        : "https://api.bitget.com/api/mix/v1/market/candles";
    const symbol = resolveSymbol(asset, quoteAsset, "");
    const startMs = startTime;
    const endMs = endTime;
    const params = isSpot
        ? { symbol, period: interval, startTime: startMs, endTime: endMs }
        : {
              symbol,
              granularity: interval,
              startTime: startMs,
              endTime: endMs,
              productType: "umcbl",
          };

    throwIfAborted(signal);
    const res = await fetch(baseUrl + buildQuery(params), { signal });
    if (!res.ok) {
        throw new Error(`Bitget error ${res.status}`);
    }
    const json = (await res.json()) as { data?: string[][]; code?: string };
    const data = json.data ?? [];
    if (!Array.isArray(data)) return [];
    return data.map((k) => {
        const start = Number(k[0]);
        return {
            start,
            open: Number(k[1]),
            high: Number(k[2]),
            low: Number(k[3]),
            close: Number(k[4]),
            volume: Number(k[5]) || 0,
            end: start + baseIntervalMs,
            trades: 0,
            asset,
            interval: intervalLabel,
        };
    });
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

async function fetchMexcCandles({
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
    if (source.market === "spot") {
        const symbol = resolveSymbol(asset, quoteAsset, "");
        const baseUrl = "https://api.mexc.com/api/v3/klines";
        const limit = 1000;
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
                throw new Error(`MEXC spot error ${res.status}`);
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

    const symbol = resolveSymbol(asset, quoteAsset, "_");
    const baseUrl = `https://contract.mexc.com/api/v1/contract/kline/${symbol}`;
    const startSec = Math.floor(startTime / 1000);
    const endSec = Math.floor(endTime / 1000);
    const url =
        baseUrl + buildQuery({ interval, start: startSec, end: endSec });

    throwIfAborted(signal);
    const res = await fetch(url, { signal });
    if (!res.ok) {
        throw new Error(`MEXC futures error ${res.status}`);
    }
    const json = (await res.json()) as {
        data?: {
            time: number[];
            open: number[];
            high: number[];
            low: number[];
            close: number[];
            vol: number[];
        };
    };
    const data = json.data;
    if (!data || !Array.isArray(data.time)) return [];
    const out: CandleData[] = [];
    for (let i = 0; i < data.time.length; i++) {
        const start = data.time[i] * 1000;
        out.push({
            start,
            open: Number(data.open[i]),
            high: Number(data.high[i]),
            low: Number(data.low[i]),
            close: Number(data.close[i]),
            volume: Number(data.vol[i]) || 0,
            end: start + baseIntervalMs,
            trades: 0,
            asset,
            interval: intervalLabel,
        });
    }
    return out;
}
