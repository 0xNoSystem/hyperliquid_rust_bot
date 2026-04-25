import { formatVolume } from "./chart/utils.ts";

export const indicatorKinds: IndicatorName[] = [
    "obv",
    "rsi",
    "smaOnRsi",
    "stochRsi",
    "adx",
    "atr",
    "ema",
    "dema",
    "tema",
    "emaCross",
    "macd",
    "ichimoku",
    "sma",
    "bollingerBands",
    "roc",
    "histVolatility",
    "volMa",
    "vwapDeviation",
    "cci",
];

export type IndicatorName =
    | "obv"
    | "volMa"
    | "rsi"
    | "smaOnRsi"
    | "stochRsi"
    | "adx"
    | "atr"
    | "ema"
    | "dema"
    | "tema"
    | "emaCross"
    | "sma"
    | "histVolatility"
    | "vwapDeviation"
    | "cci"
    | "ichimoku"
    | "macd"
    | "roc"
    | "bollingerBands";

export type IndicatorKind =
    | "obv"
    | { histVolatility: number }
    | { volMa: number }
    | { rsi: number }
    | { dema: number }
    | { tema: number }
    | { vwapDeviation: number }
    | { cci: number }
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
    | { macd: { fast: number; slow: number; signal: number } }
    | {
          ichimoku: { tenkan: number; kijun: number; senkou_b: number };
      }
    | {
          bollingerBands: { periods: number; std_multiplier_x100: number };
      }
    | { roc: number }
    | { sma: number };

export const indicatorParamLabels: Record<IndicatorName, string[]> = {
    obv: [],
    histVolatility: ["Periods"],
    volMa: ["Periods"],
    rsi: ["Periods"],
    dema: ["Periods"],
    tema: ["Periods"],
    vwapDeviation: ["Periods"],
    cci: ["Periods"],
    smaOnRsi: ["Periods", "Smoothing"],
    stochRsi: ["Periods", "kSmoothing", "dSmoothing"],
    adx: ["Periods", "DiLength"],
    atr: ["Periods"],
    ema: ["Periods"],
    emaCross: ["Short", "Long"],
    macd: ["Fast", "Slow", "Signal"],
    ichimoku: ["Tenkan", "Kijun", "Senkou B"],
    bollingerBands: ["Periods", "StdMultiplier x100"],
    roc: ["Periods"],
    sma: ["Periods"],
};

export const indicatorDefaults: Record<
    IndicatorName,
    [number, number, number]
> = {
    rsi: [14, 14, 9],
    smaOnRsi: [14, 9, 9],
    stochRsi: [14, 3, 3],
    macd: [12, 26, 9],
    roc: [12, 14, 9],
    cci: [20, 14, 9],
    adx: [14, 14, 9],
    atr: [14, 14, 9],
    ema: [9, 14, 9],
    emaCross: [9, 21, 9],
    sma: [9, 14, 9],
    dema: [9, 14, 9],
    tema: [9, 14, 9],
    ichimoku: [9, 26, 52],
    bollingerBands: [20, 200, 9],
    histVolatility: [20, 14, 9],
    obv: [14, 14, 9],
    volMa: [20, 14, 9],
    vwapDeviation: [20, 14, 9],
};

export type EngineView = "idle" | "armed" | "opening" | "closing" | "open";

export interface BackendMarketInfo {
    asset: string;
    lev: number;
    strategyName: string;
    price: number;
    margin: number;
    pnl: number;
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
    strategyName: string;
    isPaused: boolean;
    indicators: indicatorData[];
    log: string[];
    trades: TradeInfo[];
    position: OpenPositionLocal | null;
    engineState: EngineView;
}

export interface ScriptLog {
    asset: string;
    msg: string;
}

export interface indicatorData {
    id: IndexId;
    value?: Value;
}

export type Value =
    | { rsiValue: number }
    | { stochRsiValue: { k: number; d: number } }
    | { emaValue: number }
    | { demaValue: number }
    | { temaValue: number }
    | { obvValue: number }
    | { vwapDeviationValue: number }
    | { cciValue: number }
    | {
          ichimokuValue: {
              tenkan: number;
              kijun: number;
              span_a: number;
              span_b: number;
              chikou: number;
          };
      }
    | { emaCrossValue: { short: number; long: number; trend: boolean } }
    | {
          macdValue: {
              macd: number;
              signal: number;
              histogram: number;
          };
      }
    | { smaValue: number }
    | { smaRsiValue: number }
    | { rocValue: number }
    | {
          bollingerValue: {
              upper: number;
              mid: number;
              lower: number;
              width: number;
          };
      }
    | { adxValue: number }
    | { atrValue: number }
    | { histVolatilityValue: number }
    | { volumeMaValue: number };

export function get_value(v: Value, decimals: number): string {
    if (!v) return "No value";
    if ("histVolatilityValue" in v)
        return `${v.histVolatilityValue.toFixed(2)}`;
    if ("volumeMaValue" in v) return formatVolume(v.volumeMaValue);
    if ("obvValue" in v) return formatVolume(v.obvValue);
    if ("rsiValue" in v) return `${v.rsiValue.toFixed(2)}`;
    if ("vwapDeviationValue" in v) return `${v.vwapDeviationValue.toFixed(2)}`;
    if ("cciValue" in v) return `${v.cciValue.toFixed(2)}`;
    if ("demaValue" in v) return `${v.demaValue.toFixed(decimals)}`;
    if ("temaValue" in v) return `${v.temaValue.toFixed(decimals)}`;
    if ("stochRsiValue" in v)
        return `K=${v.stochRsiValue.k.toFixed(2)}, D=${v.stochRsiValue.d.toFixed(2)}`;
    if ("emaValue" in v) return `${v.emaValue.toFixed(decimals)}`;
    if ("macdValue" in v)
        return `M=${v.macdValue.macd.toFixed(decimals)}, S=${v.macdValue.signal.toFixed(decimals)}, H=${v.macdValue.histogram.toFixed(decimals)}`;
    if ("ichimokuValue" in v)
        return `T=${v.ichimokuValue.tenkan.toFixed(decimals)}, K=${v.ichimokuValue.kijun.toFixed(decimals)}, A=${v.ichimokuValue.span_a.toFixed(decimals)}, B=${v.ichimokuValue.span_b.toFixed(decimals)}, C=${v.ichimokuValue.chikou.toFixed(decimals)}`;
    if ("bollingerValue" in v)
        return `U=${v.bollingerValue.upper.toFixed(decimals)}, M=${v.bollingerValue.mid.toFixed(decimals)}, L=${v.bollingerValue.lower.toFixed(decimals)}, W=${v.bollingerValue.width.toFixed(2)}%`;
    if ("emaCrossValue" in v)
        return `short=${v.emaCrossValue.short.toFixed(decimals)}, long=${v.emaCrossValue.long.toFixed(decimals)}, trend=${v.emaCrossValue.trend ? "↑" : "↓"}`;
    if ("smaValue" in v) return `${v.smaValue.toFixed(decimals)}`;
    if ("smaRsiValue" in v) return `${v.smaRsiValue.toFixed(2)}`;
    if ("rocValue" in v) return `${v.rocValue.toFixed(2)}%`;
    if ("adxValue" in v) return `${v.adxValue.toFixed(2)}`;
    if ("atrValue" in v) return `${v.atrValue.toFixed(decimals)}`;
    return "Unknown";
}

export function get_params(k: IndicatorKind): string {
    if (k === "obv") {
        return "No params";
    }
    if ("histVolatility" in k) {
        return `Periods: ${k.histVolatility}`;
    }

    if ("volMa" in k) {
        return `Periods: ${k.volMa}`;
    }
    if ("dema" in k) {
        return `Periods: ${k.dema}`;
    }
    if ("tema" in k) {
        return `Periods: ${k.tema}`;
    }
    if ("vwapDeviation" in k) {
        return `Periods: ${k.vwapDeviation}`;
    }
    if ("cci" in k) {
        return `Periods: ${k.cci}`;
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
    if ("macd" in k) {
        const { fast, slow, signal } = k.macd;
        return `Fast: ${fast}, Slow: ${slow}, Signal: ${signal}`;
    }
    if ("ichimoku" in k) {
        const { tenkan, kijun, senkou_b } = k.ichimoku;
        return `Tenkan: ${tenkan}, Kijun: ${kijun}, Senkou B: ${senkou_b}`;
    }
    if ("bollingerBands" in k) {
        const { periods, std_multiplier_x100 } = k.bollingerBands;
        return `Periods: ${periods}, StdMultiplier x100: ${std_multiplier_x100}`;
    }
    if ("roc" in k) {
        return `Periods: ${k.roc}`;
    }
    if ("sma" in k) {
        return `Periods: ${k.sma}`;
    }
    return "Unknown";
}

export function indicator_name(kind: IndicatorKind): IndicatorName {
    if (typeof kind === "string") return kind;
    return Object.keys(kind)[0] as IndicatorName;
}

export type Decomposed = {
    asset: string;
    kind: IndicatorKind;
    timeframe: TimeFrame;
    value?: Value;
};

export function decompose(ind: indicatorData): Decomposed {
    const [asset, kind, timeframe] = ind.id;
    return { asset, kind, timeframe, value: ind.value };
}

export type IndexId = [string, IndicatorKind, TimeFrame];

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

export interface AddMarketInfo {
    asset: string;
    marginAlloc: MarginAllocation;
    lev: number;
    strategyId?: string | null;
    config?: IndexId[];
}

export type BackendLoadSessionPayload =
    | [BackendMarketInfo[], assetMeta[]]
    | assetMeta[];

export type MarketStream =
    | { price: { asset: string; price: number } }
    | { indicators: { asset: string; data: indicatorData[] } };

export type BackendStatus = "online" | "offline" | "shutdown";

export type BacktestProgress =
    | { kind: "initializing" }
    | { kind: "loadingCandles"; loaded: number; total: number }
    | { kind: "warmingEngine"; loaded: number; total: number }
    | { kind: "simulating"; processed: number; total: number }
    | { kind: "finalizing" }
    | { kind: "done" }
    | { kind: "failed"; message: string };

export interface BacktestSource {
    exchange: "binance" | "bybit" | "htx";
    market: "spot" | "futures";
    quoteAsset: "USDT" | "USDC" | string;
}

export interface BacktestConfig {
    asset: string;
    source: BacktestSource;
    strategyId: string;
    resolution: TimeFrame;
    margin: number;
    lev: number;
    takerFeeBps: number;
    makerFeeBps: number;
    fundingRateBpsPer8h: number;
    startTime: number;
    endTime: number;
    snapshotIntervalCandles: number;
    maxEquityPoints?: number;
    maxSnapshots?: number;
}

export interface CandlePoint {
    openTime: number;
    closeTime: number;
    open: number;
    high: number;
    low: number;
    close: number;
    volume: number;
}

export interface EquityPoint {
    ts: number;
    equity: number;
    balance: number;
    upnl: number;
}

export type SnapshotReason =
    | "open"
    | "reduce"
    | "flatten"
    | "close"
    | "forceClose"
    | "cancelResting"
    | "fill"
    | "interval";

export interface PositionSnapshot {
    id: number;
    ts: number;
    candle: CandlePoint;
    upnl: number;
    balance: number;
    equity: number;
    reason: SnapshotReason;
    engineState: EngineView;
    indicators: indicatorData[];
    position: OpenPositionLocal | null;
}

export interface BacktestSummary {
    initialEquity: number;
    finalEquity: number;
    netPnl: number;
    returnPct: number;
    maxDrawdownAbs: number;
    maxDrawdownPct: number;
    totalTrades: number;
    wins: number;
    losses: number;
    winRatePct: number;
    grossProfit: number;
    grossLoss: number;
    avgWin: number;
    avgLoss: number;
    profitFactor: number | null;
    expectancy: number;
    sharpeRatio?: number | null;
}

export interface BacktestResult {
    runId: string;
    startedAt: number;
    finishedAt: number;
    candlesLoaded: number;
    candlesProcessed: number;
    config: BacktestConfig;
    summary: BacktestSummary;
    trades: TradeInfo[];
    equityCurve: EquityPoint[];
    snapshots: PositionSnapshot[];
}

/** Lightweight row from `backtest_runs` table — used for history list */
export interface BacktestRunEntry {
    id: string;
    pubkey: string;
    strategyId: string;
    strategyName: string;
    asset: string;
    resolution: string;
    exchange: string;
    market: string;
    margin: number;
    lev: number;
    startTime: number;
    endTime: number;
    netPnl: number;
    returnPct: number;
    maxDrawdownPct: number;
    totalTrades: number;
    winRatePct: number;
    profitFactor: number | null;
    sharpeRatio: number | null;
    startedAt: number;
    finishedAt: number;
    createdAt: string;
}

/** Full result from `backtest_results` table — fetched on click */
export interface BacktestResultDetail {
    id: string;
    runId: string;
    initialEquity: number;
    finalEquity: number;
    grossProfit: number;
    grossLoss: number;
    avgWin: number;
    avgLoss: number;
    expectancy: number;
    wins: number;
    losses: number;
    candlesLoaded: number;
    candlesProcessed: number;
    maxDrawdownAbs: number;
    trades: TradeInfo[];
    equityCurve: EquityPoint[];
    snapshots: PositionSnapshot[];
}

export interface BacktestProgressUpdate {
    runId: string;
    progress: BacktestProgress;
}

export interface BacktestResultUpdate {
    runId: string;
    result: BacktestResult;
}

export interface BacktestRunState {
    runId: string;
    progress: BacktestProgress[];
    latestProgress: BacktestProgress | null;
    result: BacktestResult | null;
    updatedAt: number;
}

export type Message =
    | { preconfirmMarket: string }
    | { confirmMarket: BackendMarketInfo }
    | { cancelMarket: string }
    | { marketStream: MarketStream }
    | { strategyLog: ScriptLog }
    | { updateTotalMargin: number }
    | { updateMarketMargin: assetMargin }
    | { marketInfoEdit: [string, editMarketInfo] }
    | { userError: string }
    | { backtestProgress: BacktestProgressUpdate }
    | { backtestResult: BacktestResultUpdate }
    | { loadSession: BackendLoadSessionPayload }
    | { status: BackendStatus };

export type assetMargin = [string, number];

export type editMarketInfo =
    | { lev: number }
    | { openPosition: OpenPositionLocal | null }
    | { trade: TradeInfo }
    | { engineState: EngineView }
    | { paused: boolean };

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
    totalPnl?: number;
    fees: number;
    funding: number;
    open: FillInfo;
    close: FillInfo;
    strategy?: string;
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
    obv: "OBV",
    histVolatility: "Historical Volatility",
    volMa: "Volume MA",
    rsi: "RSI",
    dema: "DEMA",
    tema: "TEMA",
    vwapDeviation: "VWAP Deviation",
    cci: "CCI",
    smaOnRsi: "SMA on RSI",
    stochRsi: "Stoch RSI",
    adx: "ADX",
    atr: "ATR",
    ema: "EMA",
    emaCross: "EMA Cross",
    macd: "MACD",
    ichimoku: "Ichimoku",
    bollingerBands: "Bollinger Bands",
    roc: "ROC",
    sma: "SMA",
};

export const indicatorColors: Record<IndicatorName, string> = {
    obv: "bg-indicator-vol-ma-bg text-indicator-vol-ma-text",
    histVolatility: "bg-indicator-hist-vol-bg text-indicator-hist-vol-text",
    volMa: "bg-indicator-vol-ma-bg text-indicator-vol-ma-text",
    rsi: "bg-indicator-rsi-bg text-indicator-rsi-text",
    dema: "bg-indicator-ema-bg text-indicator-ema-text",
    tema: "bg-indicator-ema-bg text-indicator-ema-text",
    vwapDeviation: "bg-indicator-stoch-rsi-bg text-indicator-stoch-rsi-text",
    cci: "bg-indicator-adx-bg text-indicator-adx-text",
    smaOnRsi: "bg-indicator-sma-on-rsi-bg text-indicator-sma-on-rsi-text",
    stochRsi: "bg-indicator-stoch-rsi-bg text-indicator-stoch-rsi-text",
    adx: "bg-indicator-adx-bg text-indicator-adx-text",
    atr: "bg-indicator-atr-bg text-indicator-atr-text",
    ema: "bg-indicator-ema-bg text-indicator-ema-text",
    emaCross: "bg-indicator-ema-cross-bg text-indicator-ema-cross-text",
    macd: "bg-indicator-sma-on-rsi-bg text-indicator-sma-on-rsi-text",
    ichimoku: "bg-indicator-ema-cross-bg text-indicator-ema-cross-text",
    bollingerBands: "bg-indicator-hist-vol-bg text-indicator-hist-vol-text",
    roc: "bg-indicator-atr-bg text-indicator-atr-text",
    sma: "bg-indicator-sma-bg text-indicator-sma-text",
};

export const indicatorValueColors: Record<IndicatorName, string> = {
    obv: "text-indicator-vol-ma-text",
    histVolatility: "text-indicator-hist-vol-text",
    volMa: "text-indicator-vol-ma-text",
    rsi: "text-indicator-rsi-text",
    dema: "text-indicator-ema-text",
    tema: "text-indicator-ema-text",
    vwapDeviation: "text-indicator-stoch-rsi-text",
    cci: "text-indicator-adx-text",
    smaOnRsi: "text-indicator-sma-on-rsi-text",
    stochRsi: "text-indicator-stoch-rsi-text",
    adx: "text-indicator-adx-text",
    atr: "text-indicator-atr-text",
    ema: "text-indicator-ema-text",
    emaCross: "text-indicator-ema-cross-text",
    macd: "text-indicator-sma-on-rsi-text",
    ichimoku: "text-indicator-ema-cross-text",
    bollingerBands: "text-indicator-hist-vol-text",
    roc: "text-indicator-atr-text",
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
