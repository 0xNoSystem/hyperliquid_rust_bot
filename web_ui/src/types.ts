import type { Strategy } from "./strats.ts";
import { formatVolume } from "./chart/utils.ts";

export const indicatorKinds: IndicatorName[] = [
    "rsi",
    "smaOnRsi",
    "stochRsi",
    "adx",
    "atr",
    "ema",
    "emaCross",
    "sma",
    "histVolatility",
    "volMa",
];

export type IndicatorName =
    | "volMa"
    | "rsi"
    | "smaOnRsi"
    | "stochRsi"
    | "adx"
    | "atr"
    | "ema"
    | "emaCross"
    | "sma"
    | "histVolatility";

export type IndicatorKind =
    | { histVolatility: number }
    | { volMa: number }
    | { rsi: number }
    | { smaOnRsi: { periods: number; smoothing_length: number } }
    | {
          stochRsi: {
              periods: number;
              k_smoothing?: number | null;
              d_smoothing?: number | null;
          };
      }
    | { adx: { periods: number; di_length: number } }
    | { atr: number }
    | { ema: number }
    | { emaCross: { short: number; long: number } }
    | { sma: number };

export const indicatorParamLabels: Record<IndicatorName, string[]> = {
    histVolatility: ["Periods"],
    volMa: ["Periods"],
    rsi: ["Periods"],
    smaOnRsi: ["Periods", "Smoothing"],
    stochRsi: ["Periods", "kSmoothing", "dSmoothing"],
    adx: ["Periods", "DiLength"],
    atr: ["Periods"],
    ema: ["Periods"],
    emaCross: ["Short", "Long"],
    sma: ["Periods"],
};

export type EngineView = "idle" | "armed" | "opening" | "closing" | "open";

export interface BackendMarketInfo {
    asset: string;
    lev: number;
    price: number;
    margin: number;
    pnl: number;
    strategy: Strategy;
    isPaused: boolean;
    indicators: indicatorData[];
    position: OpenPositionLocal | null;
    engineState: EngineView;
}

export interface MarketInfo {
    asset: string;
    state: "Loading" | "Ready";
    lev: number | null;
    price: number | null;
    prev: number | null;
    margin: number | null;
    pnl: number | null;
    strategy: Strategy;
    isPaused: boolean;
    indicators: indicatorData[];
    trades: TradeInfo[];
    position: OpenPositionLocal | null;
    engineState: EngineView;
}

export interface indicatorData {
    id: IndexId;
    value?: Value;
}

export type Value =
    | { rsiValue: number }
    | { stochRsiValue: { k: number; d: number } }
    | { emaValue: number }
    | { emaCrossValue: { short: number; long: number; trend: boolean } }
    | { smaValue: number }
    | { smaRsiValue: number }
    | { adxValue: number }
    | { atrValue: number }
    | { histVolatilityValue: number }
    | { volumeMaValue: number };

export function get_value(v: Value, decimals: number): string {
    if (!v) return "No value";
    if ("histVolatilityValue" in v)
        return `${v.histVolatilityValue.toFixed(2)}`;
    if ("volumeMaValue" in v) return formatVolume(v.volumeMaValue);
    if ("rsiValue" in v) return `${v.rsiValue.toFixed(2)}`;
    if ("stochRsiValue" in v)
        return `K=${v.stochRsiValue.k.toFixed(2)}, D=${v.stochRsiValue.d.toFixed(2)}`;
    if ("emaValue" in v) return `${v.emaValue.toFixed(decimals)}`;
    if ("emaCrossValue" in v)
        return `short=${v.emaCrossValue.short.toFixed(decimals)}, long=${v.emaCrossValue.long.toFixed(decimals)}, trend=${v.emaCrossValue.trend ? "↑" : "↓"}`;
    if ("smaValue" in v) return `${v.smaValue.toFixed(decimals)}`;
    if ("smaRsiValue" in v) return `${v.smaRsiValue.toFixed(2)}`;
    if ("adxValue" in v) return `${v.adxValue.toFixed(2)}`;
    if ("atrValue" in v) return `${v.atrValue.toFixed(decimals)}`;
    return "Unknown";
}

export function get_params(k: IndicatorKind): string {
    if ("histVolatility" in k) {
        return `Periods: ${k.histVolatility}`;
    }

    if ("volMa" in k) {
        return `Periods: ${k.volMa}`;
    }
    if ("rsi" in k) {
        return `Periods: ${k.rsi}`;
    }
    if ("smaOnRsi" in k) {
        const { periods, smoothing_length } = k.smaOnRsi;
        return `Periods: ${periods}, Smoothing: ${smoothing_length}`;
    }
    if ("stochRsi" in k) {
        const { periods, k_smoothing, d_smoothing } = k.stochRsi;
        return `Periods: ${periods}, kSmoothing: ${k_smoothing ?? "3"}, dSmoothing: ${d_smoothing ?? "3"}`;
    }
    if ("adx" in k) {
        const { periods, di_length } = k.adx;
        return `Periods: ${periods}, DiLength: ${di_length}`;
    }
    if ("atr" in k) {
        return `Periods: ${k.atr}`;
    }
    if ("ema" in k) {
        return `Periods: ${k.ema}`;
    }
    if ("emaCross" in k) {
        const { short, long } = k.emaCross;
        return `Short: ${short}, Long: ${long}`;
    }
    if ("sma" in k) {
        return `Periods: ${k.sma}`;
    }
    return "Unknown";
}

export type Decomposed = {
    kind: IndicatorKind;
    timeframe: TimeFrame;
    value?: Value;
};

export function decompose(ind: indicatorData): Decomposed {
    const [kind, timeframe] = ind.id;
    return { kind, timeframe, value: ind.value };
}

export type IndexId = [IndicatorKind, TimeFrame];

export type TimeFrame =
    | "min1"
    | "min3"
    | "min5"
    | "min15"
    | "min30"
    | "hour1"
    | "hour2"
    | "hour4"
    | "hour12"
    | "day1"
    | "day3"
    | "week"
    | "month";

export const TIMEFRAME_CAMELCASE: Record<string, TimeFrame> = {
    "1m": "min1",
    "3m": "min3",
    "5m": "min5",
    "15m": "min15",
    "30m": "min30",
    "1h": "hour1",
    "2h": "hour2",
    "4h": "hour4",
    "12h": "hour12",
    "1d": "day1",
    "3d": "day3",
    "1w": "week",
    "1M": "month",
};

const TIMEFRAME_SHORT: Record<TimeFrame, string> = Object.entries(
    TIMEFRAME_CAMELCASE
).reduce(
    (acc, [short, tf]) => {
        acc[tf] = short;
        return acc;
    },
    {} as Record<TimeFrame, string>
);

export function fromTimeFrame(tf: TimeFrame): string {
    return TIMEFRAME_SHORT[tf];
}

export function into(tf: string): TimeFrame {
    return TIMEFRAME_CAMELCASE[tf];
}

export type MarginAllocation = { alloc: number } | { amount: number };

export function market_add_info(m: MarketInfo): AddMarketInfo {
    const { asset, margin, lev, strategy, indicators } = m;
    const config = indicators.map((i) => i.id);

    return {
        asset,
        marginAlloc: { amount: margin ?? 0 },
        lev: lev ?? 1,
        strategy,
        config,
    };
}

export interface AddMarketInfo {
    asset: string;
    marginAlloc: MarginAllocation;
    lev: number;
    strategy: Strategy;
    config?: IndexId[];
}

export interface AddMarketProps {
    onClose: () => void;
    totalMargin: number;
    assets: assetMeta[];
}

export type BackendLoadSessionPayload =
    | [BackendMarketInfo[], assetMeta[]]
    | assetMeta[];

export type MarketStream =
    | { price: { asset: string; price: number } }
    | { indicators: { asset: string; data: indicatorData[] } };

export type BackendStatus = "online" | "offline" | "shutdown";

export type Message =
    | { preconfirmMarket: string }
    | { confirmMarket: BackendMarketInfo }
    | { cancelMarket: string }
    | { marketStream: MarketStream }
    | { updateTotalMargin: number }
    | { updateMarketMargin: assetMargin }
    | { marketInfoEdit: [string, editMarketInfo] }
    | { userError: string }
    | { loadSession: BackendLoadSessionPayload }
    | { status: BackendStatus };

export type assetMargin = [string, number];

export type editMarketInfo =
    | { lev: number }
    | { openPosition: OpenPositionLocal | null }
    | { trade: TradeInfo }
    | { engineState: EngineView };

export type Side = "long" | "short";

export type PositionOp = "openLong" | "openShort" | "close";

export type FillType =
    | "market"
    | "limit"
    | "liquidation"
    | { trigger: TriggerKind };

export type TriggerKind = "tp" | "sl";

export interface FillInfo {
    time: number; // unix ms
    price: number;
    fillType: FillType;
}

export interface TradeInfo {
    side: Side;
    size: number;
    pnl: number;
    fees: number;
    funding: number;
    open: FillInfo;
    close: FillInfo;
}

export interface OpenPositionLocal {
    openTime: number; // unix ms
    size: number;
    entryPx: number;
    side: Side;
    fees: number;
    funding: number;
    realisedPnl: number;
    fillType: FillType;
}

export const indicatorLabels: Record<IndicatorName, string> = {
    histVolatility: "Historical Volatility",
    volMa: "Volume MA",
    rsi: "RSI",
    smaOnRsi: "SMA on RSI",
    stochRsi: "Stoch RSI",
    adx: "ADX",
    atr: "ATR",
    ema: "EMA",
    emaCross: "EMA Cross",
    sma: "SMA",
};

export const indicatorColors: Record<IndicatorName, string> = {
    histVolatility: "bg-indicator-hist-vol-bg text-indicator-hist-vol-text",
    volMa: "bg-indicator-vol-ma-bg text-indicator-vol-ma-text",
    rsi: "bg-indicator-rsi-bg text-indicator-rsi-text",
    smaOnRsi: "bg-indicator-sma-on-rsi-bg text-indicator-sma-on-rsi-text",
    stochRsi: "bg-indicator-stoch-rsi-bg text-indicator-stoch-rsi-text",
    adx: "bg-indicator-adx-bg text-indicator-adx-text",
    atr: "bg-indicator-atr-bg text-indicator-atr-text",
    ema: "bg-indicator-ema-bg text-indicator-ema-text",
    emaCross: "bg-indicator-ema-cross-bg text-indicator-ema-cross-text",
    sma: "bg-indicator-sma-bg text-indicator-sma-text",
};

export const indicatorValueColors: Record<IndicatorName, string> = {
    histVolatility: "text-indicator-hist-vol-text",
    volMa: "text-indicator-vol-ma-text",
    rsi: "text-indicator-rsi-text",
    smaOnRsi: "text-indicator-sma-on-rsi-text",
    stochRsi: "text-indicator-stoch-rsi-text",
    adx: "text-indicator-adx-text",
    atr: "text-indicator-atr-text",
    ema: "text-indicator-ema-text",
    emaCross: "text-indicator-ema-cross-text",
    sma: "text-indicator-sma-text",
};

export interface assetMeta {
    name: string;
    szDecimals: number;
    maxLeverage: number;
}

export const TF_TO_MS: Record<TimeFrame, number> = {
    min1: 60_000,
    min3: 3 * 60_000,
    min5: 5 * 60_000,
    min15: 15 * 60_000,
    min30: 30 * 60_000,
    hour1: 60 * 60_000,
    hour2: 2 * 60 * 60_000,
    hour4: 4 * 60 * 60_000,
    hour12: 12 * 60 * 60_000,
    day1: 24 * 60 * 60_000,
    day3: 3 * 24 * 60 * 60_000,
    week: 7 * 24 * 60 * 60_000,
    month: 30 * 24 * 60 * 60_000,
};

export const sanitizeAsset = (asset: string) => {
    if (asset[0] == "k") {
        return asset.slice(1);
    }
    return asset;
};

export function computeUPnL(
    position: OpenPositionLocal,
    marketPrice: number,
    leverage: number
): [number, number] {
    const direction = position.side === "long" ? 1 : -1;

    const upnl = (marketPrice - position.entryPx) * direction * position.size;

    const margin = (position.entryPx * position.size) / leverage;

    const relativeChange = upnl / margin;

    return [upnl, relativeChange];
}

export const formatPrice = (n: number) => {
    if (n > 1 && n < 2) return n.toFixed(4);
    if (n < 1) return n.toFixed(6);
    return n.toFixed(2);
};

export function num(n: number, d = 2) {
    return Number.isFinite(n) ? n.toFixed(d) : "—";
}
