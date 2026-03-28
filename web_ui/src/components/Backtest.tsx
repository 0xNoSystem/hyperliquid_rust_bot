import { useState, useEffect, useMemo, useCallback, useRef } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
    TIMEFRAME_CAMELCASE,
    TF_TO_MS,
    fromTimeFrame,
    formatPrice,
    num,
    sanitizeAsset,
} from "../types";
import type { BacktestProgress, BacktestResult, TimeFrame } from "../types";
import ChartContainer from "../chart/ChartContainer";
import { isTimeframeSupported } from "../chart/dataSources";
import { loadCandles } from "../chart/loader";
import { API_URL } from "../consts";
import {
    DEFAULT_DATA_SOURCE,
    type DataSource,
    type ExchangeId,
    type MarketType,
} from "../chart/types";
import {
    EXCHANGE_OPTIONS,
    MARKET_OPTIONS,
    getMarketsForExchange,
} from "../chart/providers";
import AssetIcon from "../chart/visual/AssetIcon";
import { formatUTC } from "../chart/utils";
import type { CandleData } from "../chart/utils";
import { useChartContext } from "../chart/ChartContextStore";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { useAuth } from "../context/AuthContextStore";
import type { Strategy } from "../strats";

type RangePreset = "24H" | "7D" | "30D" | "YTD" | "CUSTOM";

const RANGE_PRESETS: { id: RangePreset; label: string }[] = [
    { id: "24H", label: "24H" },
    { id: "7D", label: "7D" },
    { id: "30D", label: "30D" },
    { id: "YTD", label: "YTD" },
    { id: "CUSTOM", label: "Custom" },
];

const PRESET_DEFAULT_TF: Partial<Record<RangePreset, TimeFrame>> = {
    "24H": "hour1",
    "7D": "hour1",
    "30D": "hour4",
    YTD: "day1",
};

const TIMEFRAME_ORDER = Object.values(TIMEFRAME_CAMELCASE) as TimeFrame[];

type CustomDateParts = {
    year: number;
    month: number; // 1-based
    day: number;
    time: string; // HH:MM (24h)
};

const CURRENT_YEAR = new Date().getUTCFullYear();
const YEARS = Array.from(
    { length: CURRENT_YEAR - 2016 + 1 },
    (_, idx) => 2016 + idx
);
const MONTHS = [
    { value: 1, label: "Jan" },
    { value: 2, label: "Feb" },
    { value: 3, label: "Mar" },
    { value: 4, label: "Apr" },
    { value: 5, label: "May" },
    { value: 6, label: "Jun" },
    { value: 7, label: "Jul" },
    { value: 8, label: "Aug" },
    { value: 9, label: "Sep" },
    { value: 10, label: "Oct" },
    { value: 11, label: "Nov" },
    { value: 12, label: "Dec" },
];

const QUOTE_ASSET_OPTIONS = ["USDT", "USDC"] as const;
type QuoteAsset = (typeof QUOTE_ASSET_OPTIONS)[number];
const BACKTEST_RESOLUTION: TimeFrame = "min1";

type BacktestExchange = "binance" | "bybit" | "mexc" | "htx" | "coinbase";

type BacktestRunRequestPayload = {
    runId: string;
    config: {
        asset: string;
        source: {
            exchange: BacktestExchange;
            market: MarketType;
            quoteAsset: QuoteAsset;
        };
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
    };
    warmupCandles: number;
};

type BacktestRunResponsePayload = {
    runId: string;
    result: BacktestResult;
    progress: BacktestProgress[];
};

type BacktestRunErrorPayload = {
    runId?: string;
    message?: string;
    progress?: BacktestProgress[];
};

function toBacktestExchange(exchange: ExchangeId): BacktestExchange | null {
    switch (exchange) {
        case "binance":
        case "bybit":
        case "mexc":
        case "htx":
        case "coinbase":
            return exchange;
        default:
            return null;
    }
}

function formatUtcMinute(ts: number): string {
    return new Date(ts).toISOString().slice(0, 16).replace("T", " ") + " UTC";
}

function getDaysInMonth(year: number, month: number): number {
    return new Date(Date.UTC(year, month, 0)).getUTCDate();
}

function dateToParts(date: Date): CustomDateParts {
    const year = date.getUTCFullYear();
    const month = date.getUTCMonth() + 1;
    const day = date.getUTCDate();
    const hours = String(date.getUTCHours()).padStart(2, "0");
    const minutes = String(date.getUTCMinutes()).padStart(2, "0");
    return { year, month, day, time: `${hours}:${minutes}` };
}

function partsToMs(parts: CustomDateParts): number {
    const [hours, minutes] = parts.time.split(":").map((n) => Number(n) || 0);
    return Date.UTC(parts.year, parts.month - 1, parts.day, hours, minutes);
}

function sanitizeTime(value: string): string {
    if (!value) return "00:00";
    const [hours = "0", minutes = "0"] = value.split(":");
    const h = Math.min(23, Math.max(0, Number(hours)));
    const m = Math.min(59, Math.max(0, Number(minutes)));
    return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

function normalizeParts(parts: CustomDateParts): CustomDateParts {
    const maxDay = getDaysInMonth(parts.year, parts.month);
    const day = Math.min(parts.day, maxDay);
    return { ...parts, day, time: sanitizeTime(parts.time) };
}

function buildCustomRangeISO(
    startParts: CustomDateParts,
    endParts: CustomDateParts
) {
    const now = Date.now();
    const startMsRaw = partsToMs(startParts);
    const endMsRaw = partsToMs(endParts);

    const startMs = Math.min(startMsRaw, now);
    let endMs = Math.min(endMsRaw, now);

    if (endMs <= startMs) {
        endMs = Math.min(now, startMs + 60 * 60 * 1000);
    }

    return {
        start: new Date(startMs).toISOString().slice(0, 16),
        end: new Date(endMs).toISOString().slice(0, 16),
    };
}

type BacktestContentProps = {
    routeAsset?: string;
};

// -----------------------
// Backtest Content
// -----------------------
function BacktestContent({ routeAsset }: BacktestContentProps) {
    const nav = useNavigate();
    const { startTime, endTime, setTimeRange, intervalStartX, intervalEndX } =
        useChartContext();
    const { universe, backtestRuns, strategies } = useWebSocketContext();
    const { token } = useAuth();
    const activeAsset = routeAsset ?? "";
    const defaultStartParts = useMemo(
        () => dateToParts(new Date(Date.now() - 7 * 24 * 60 * 60 * 1000)),
        []
    );
    const defaultEndParts = useMemo(() => dateToParts(new Date()), []);

    const [timeframe, setTimeframe] = useState<TimeFrame>("day1");
    const [intervalOn, setIntervalOn] = useState(false);
    const [candleData, setCandleData] = useState<CandleData[]>([]);
    const [showDatePicker, setShowDatePicker] = useState(true);
    const [selectedExchange, setSelectedExchange] = useState<ExchangeId>(
        DEFAULT_DATA_SOURCE.exchange
    );
    const [selectedMarket, setSelectedMarket] = useState<MarketType>(
        DEFAULT_DATA_SOURCE.market
    );
    const [selectedStrategy, setSelectedStrategy] = useState<Strategy | null>(
        null
    );
    const [quoteAsset, setQuoteAsset] = useState<QuoteAsset>("USDT");
    const [margin, setMargin] = useState(10_000);
    const [lev, setLev] = useState(8);
    const [warmupCandles, setWarmupCandles] = useState(5_000);
    const [takerFeeBps, setTakerFeeBps] = useState(3);
    const [makerFeeBps, setMakerFeeBps] = useState(1);
    const [fundingRateBpsPer8h, setFundingRateBpsPer8h] = useState(0);
    const [isSubmittingBacktest, setIsSubmittingBacktest] = useState(false);
    const [backtestError, setBacktestError] = useState<string | null>(null);
    const [activeRunId, setActiveRunId] = useState<string | null>(null);
    const [httpBacktestResult, setHttpBacktestResult] =
        useState<BacktestResult | null>(null);
    const requestIdRef = useRef(0);
    const abortControllerRef = useRef<AbortController | null>(null);
    const backtestAbortRef = useRef<AbortController | null>(null);

    const [rangePreset, setRangePreset] = useState<RangePreset>("30D");
    const [customStartParts, setCustomStartParts] =
        useState<CustomDateParts>(defaultStartParts);
    const [customEndParts, setCustomEndParts] =
        useState<CustomDateParts>(defaultEndParts);
    const [committedStartParts, setCommittedStartParts] =
        useState<CustomDateParts>(defaultStartParts);
    const [committedEndParts, setCommittedEndParts] =
        useState<CustomDateParts>(defaultEndParts);
    const updateStartParts = (updates: Partial<CustomDateParts>) => {
        setCustomStartParts((prev) => normalizeParts({ ...prev, ...updates }));
    };
    const updateEndParts = (updates: Partial<CustomDateParts>) => {
        setCustomEndParts((prev) => normalizeParts({ ...prev, ...updates }));
    };
    const confirmCustomRange = () => {
        const range = buildCustomRangeISO(customStartParts, customEndParts);
        setCommittedStartParts(customStartParts);
        setCommittedEndParts(customEndParts);
        const startMs = new Date(range.start).getTime();
        const endMs = new Date(range.end).getTime();
        if (!Number.isNaN(startMs) && !Number.isNaN(endMs)) {
            setTimeRange(startMs, endMs);
        }
    };

    const applyPresetTimeRange = useCallback(
        (preset: RangePreset) => {
            const now = Date.now();
            switch (preset) {
                case "24H":
                    setTimeRange(now - 24 * 60 * 60 * 1000, now);
                    break;
                case "7D":
                    setTimeRange(now - 7 * 24 * 60 * 60 * 1000, now);
                    break;
                case "30D":
                    setTimeRange(now - 30 * 24 * 60 * 60 * 1000, now);
                    break;
                case "YTD": {
                    const current = new Date();
                    const startOfYear = Date.UTC(
                        current.getUTCFullYear(),
                        0,
                        1
                    );
                    setTimeRange(startOfYear, now);
                    break;
                }
                default:
                    // CUSTOM handled separately
                    break;
            }
        },
        [setTimeRange]
    );

    const handlePresetSelect = (preset: RangePreset) => {
        if (preset === rangePreset) return;
        setRangePreset(preset);

        const tfForPreset = PRESET_DEFAULT_TF[preset];
        if (tfForPreset) {
            setTimeframe(tfForPreset);
        }

        if (preset !== "CUSTOM") {
            applyPresetTimeRange(preset);
        }
    };

    useEffect(() => {
        const markets = getMarketsForExchange(selectedExchange);
        if (!markets.includes(selectedMarket)) {
            setSelectedMarket(markets[0] ?? DEFAULT_DATA_SOURCE.market);
        }
    }, [selectedExchange, selectedMarket]);

    const selectedDataSource = useMemo<DataSource>(
        () => ({
            exchange: selectedExchange,
            market: selectedMarket,
        }),
        [selectedExchange, selectedMarket]
    );

    const supportedTimeframes = useMemo(
        () =>
            TIMEFRAME_ORDER.filter((tf) =>
                isTimeframeSupported(selectedDataSource, tf)
            ),
        [selectedDataSource]
    );

    const selectedExchangeMarkets = useMemo(
        () => getMarketsForExchange(selectedExchange),
        [selectedExchange]
    );

    const selectedBacktestExchange = useMemo(
        () => toBacktestExchange(selectedExchange),
        [selectedExchange]
    );

    const backtestWindow = useMemo(() => {
        const rawStart =
            intervalOn && intervalStartX !== null ? intervalStartX : startTime;
        const rawEnd =
            intervalOn && intervalEndX !== null ? intervalEndX : endTime;

        if (!Number.isFinite(rawStart) || !Number.isFinite(rawEnd)) {
            return null;
        }

        const start = Math.floor(Math.min(rawStart, rawEnd));
        const end = Math.floor(Math.max(rawStart, rawEnd));
        if (start <= 0 || end <= start) {
            return null;
        }

        return { start, end };
    }, [intervalOn, intervalStartX, intervalEndX, startTime, endTime]);

    const backtestWindowLabel = useMemo(() => {
        if (!backtestWindow) return "Invalid interval";
        return `${formatUtcMinute(backtestWindow.start)} -> ${formatUtcMinute(backtestWindow.end)}`;
    }, [backtestWindow]);

    const canRunBacktest = useMemo(() => {
        if (!routeAsset) return false;
        if (!selectedBacktestExchange) return false;
        if (!backtestWindow) return false;
        if (!Number.isFinite(margin) || margin <= 0) return false;
        if (!Number.isFinite(lev) || lev < 1) return false;
        if (!Number.isFinite(warmupCandles) || warmupCandles < 0) return false;
        if (isSubmittingBacktest) return false;
        return true;
    }, [
        routeAsset,
        selectedBacktestExchange,
        backtestWindow,
        margin,
        lev,
        warmupCandles,
        isSubmittingBacktest,
    ]);

    const activeRun = useMemo(
        () => (activeRunId ? (backtestRuns[activeRunId] ?? null) : null),
        [activeRunId, backtestRuns]
    );

    const resultToRender = activeRun?.result ?? httpBacktestResult;
    const latestProgress = activeRun?.latestProgress ?? null;
    const resultViewState = useMemo<"nothing" | "loading" | "result">(() => {
        if (resultToRender) return "result";
        if (isSubmittingBacktest) return "loading";
        if (
            activeRunId &&
            latestProgress &&
            latestProgress.kind !== "done" &&
            latestProgress.kind !== "failed"
        ) {
            return "loading";
        }
        return "nothing";
    }, [resultToRender, isSubmittingBacktest, activeRunId, latestProgress]);

    const progressLabel = useMemo(() => {
        if (!latestProgress) return null;
        switch (latestProgress.kind) {
            case "loadingCandles":
                return `Loading candles: ${latestProgress.loaded}/${latestProgress.total}`;
            case "warmingEngine":
                return `Warming engine: ${latestProgress.loaded}/${latestProgress.total}`;
            case "simulating":
                return `Simulating: ${latestProgress.processed}/${latestProgress.total}`;
            case "initializing":
                return "Initializing backtest...";
            case "finalizing":
                return "Finalizing result...";
            case "done":
                return "Backtest done.";
            case "failed":
                return `Backtest failed: ${latestProgress.message}`;
            default:
                return null;
        }
    }, [latestProgress]);

    const runBacktest = useCallback(async () => {
        setBacktestError(null);
        setHttpBacktestResult(null);

        if (!routeAsset) {
            setBacktestError("Select an asset before running the backtest.");
            return;
        }
        if (!selectedBacktestExchange) {
            setBacktestError(
                "Selected exchange is not supported by backend backtesting yet."
            );
            return;
        }
        if (!backtestWindow) {
            setBacktestError("Backtest interval is invalid.");
            return;
        }

        const clampedLev = Math.max(1, Math.min(100, Math.floor(lev)));
        const clampedWarmup = Math.max(0, Math.floor(warmupCandles));
        const runId = `bt-${sanitizeAsset(routeAsset).toLowerCase()}-${Date.now()}`;
        setActiveRunId(runId);

        const payload: BacktestRunRequestPayload = {
            runId,
            config: {
                asset: sanitizeAsset(routeAsset).toUpperCase(),
                source: {
                    exchange: selectedBacktestExchange,
                    market: selectedMarket,
                    quoteAsset: quoteAsset,
                },
                strategyId: selectedStrategy?.id ?? "",
                resolution: BACKTEST_RESOLUTION,
                margin,
                lev: clampedLev,
                takerFeeBps: Math.max(0, Math.floor(takerFeeBps)),
                makerFeeBps: Math.max(0, Math.floor(makerFeeBps)),
                fundingRateBpsPer8h,
                startTime: backtestWindow.start,
                endTime: backtestWindow.end,
                snapshotIntervalCandles: 10,
            },
            warmupCandles: clampedWarmup,
        };

        backtestAbortRef.current?.abort();
        const controller = new AbortController();
        backtestAbortRef.current = controller;
        setIsSubmittingBacktest(true);

        try {
            const headers: Record<string, string> = {
                "Content-Type": "application/json",
            };
            if (token) headers["Authorization"] = `Bearer ${token}`;
            const res = await fetch(`${API_URL}/backtest`, {
                method: "POST",
                headers,
                body: JSON.stringify(payload),
                signal: controller.signal,
            });

            if (!res.ok) {
                let message = `Backtest request failed (${res.status})`;
                try {
                    const err = (await res.json()) as BacktestRunErrorPayload;
                    if (err?.message) {
                        message = err.message;
                    }
                    if (err?.runId) {
                        setActiveRunId(err.runId);
                    }
                } catch {
                    // no-op, fallback status message
                }
                throw new Error(message);
            }

            const data = (await res.json()) as BacktestRunResponsePayload;
            setActiveRunId(data.runId || runId);
            setHttpBacktestResult(data.result);
            console.info("Backtest completed", data);
        } catch (err) {
            if (controller.signal.aborted) return;
            setBacktestError(
                err instanceof Error
                    ? err.message
                    : "Failed to run backtest request."
            );
        } finally {
            if (backtestAbortRef.current === controller) {
                backtestAbortRef.current = null;
            }
            setIsSubmittingBacktest(false);
        }
    }, [
        token,
        routeAsset,
        selectedBacktestExchange,
        backtestWindow,
        lev,
        warmupCandles,
        selectedMarket,
        quoteAsset,
        selectedStrategy,
        margin,
        takerFeeBps,
        makerFeeBps,
        fundingRateBpsPer8h,
    ]);

    useEffect(() => {
        if (supportedTimeframes.length === 0) return;
        if (supportedTimeframes.includes(timeframe)) return;

        const currentMs = TF_TO_MS[timeframe];
        let closest = supportedTimeframes[0];
        let bestDiff = Math.abs(TF_TO_MS[closest] - currentMs);
        for (const candidate of supportedTimeframes) {
            const diff = Math.abs(TF_TO_MS[candidate] - currentMs);
            if (diff < bestDiff) {
                bestDiff = diff;
                closest = candidate;
            }
        }
        if (closest !== timeframe) {
            setTimeframe(closest);
        }
    }, [supportedTimeframes, timeframe]);

    const startDayOptions = getDaysInMonth(
        customStartParts.year,
        customStartParts.month
    );
    const endDayOptions = getDaysInMonth(
        customEndParts.year,
        customEndParts.month
    );
    const customRows = [
        {
            label: "Start",
            parts: customStartParts,
            update: updateStartParts,
            dayCount: startDayOptions,
        },
        {
            label: "End",
            parts: customEndParts,
            update: updateEndParts,
            dayCount: endDayOptions,
        },
    ] as const;

    const isCustomDirty = useMemo(() => {
        if (rangePreset !== "CUSTOM") return false;
        const partsEqual = (a: CustomDateParts, b: CustomDateParts) =>
            a.year === b.year &&
            a.month === b.month &&
            a.day === b.day &&
            a.time === b.time;

        return (
            !partsEqual(customStartParts, committedStartParts) ||
            !partsEqual(customEndParts, committedEndParts)
        );
    }, [
        rangePreset,
        customStartParts,
        committedStartParts,
        customEndParts,
        committedEndParts,
    ]);

    useEffect(() => {
        if (rangePreset !== "CUSTOM") {
            applyPresetTimeRange(rangePreset);
        }
    }, [applyPresetTimeRange, rangePreset]);

    useEffect(() => {
        if (!routeAsset) return;
        if (startTime <= 0 || endTime <= startTime) return;
        if (!isTimeframeSupported(selectedDataSource, timeframe)) return;

        const requestId = ++requestIdRef.current;
        abortControllerRef.current?.abort();
        const controller = new AbortController();
        abortControllerRef.current = controller;

        const timer = setTimeout(() => {
            (async () => {
                try {
                    const data = await loadCandles(
                        selectedDataSource,
                        timeframe,
                        startTime,
                        endTime,
                        routeAsset,
                        quoteAsset,
                        setCandleData,
                        controller.signal
                    );
                    if (requestIdRef.current === requestId) {
                        setCandleData(data);
                    }
                } catch (err) {
                    if (controller.signal.aborted) return;
                    console.error("Failed to fetch candles", err);
                }
            })();
        }, 200);

        return () => {
            clearTimeout(timer);
            controller.abort();
        };
    }, [
        startTime,
        endTime,
        timeframe,
        routeAsset,
        selectedDataSource,
        quoteAsset,
    ]);

    useEffect(() => {
        return () => {
            backtestAbortRef.current?.abort();
        };
    }, []);

    return (
        <div className="bg-ink-10 flex flex-1 flex-col pb-50">
            {/* Title */}
            <h1 className="mt-6 p-2 text-center text-3xl font-bold tracking-widest">
                STRATEGY LAB
            </h1>

            {/* Layout */}
            <div className="z-1 flex flex-grow flex-col items-center justify-between py-8">
                {/* STRATEGY (top) */}
                <div className="border-line-stronger bg-ink-60 mb-6 mb-30 w-[90%] border-2 p-4 tracking-widest">
                    <h2 className="p-2 text-center text-xl font-semibold">
                        Strategy
                    </h2>
                    <div className="grid grid-cols-1 gap-3 p-2 text-sm md:grid-cols-4">
                        <label className="text-app-text/75 flex flex-col gap-1">
                            Strategy
                            <select
                                value={selectedStrategy?.id ?? ""}
                                onChange={(e) => {
                                    const s = strategies.find(
                                        (s) => s.id === e.target.value
                                    );
                                    setSelectedStrategy(s ?? null);
                                }}
                                className="border-line-muted bg-ink-80 text-app-text rounded border px-2 py-1"
                            >
                                <option value="" disabled>
                                    -- select --
                                </option>
                                {strategies.map((s) => (
                                    <option key={s.id} value={s.id}>
                                        {s.name}
                                    </option>
                                ))}
                            </select>
                        </label>

                        <label className="text-app-text/75 flex flex-col gap-1">
                            Quote Asset
                            <select
                                value={quoteAsset}
                                onChange={(e) =>
                                    setQuoteAsset(e.target.value as QuoteAsset)
                                }
                                className="border-line-muted bg-ink-80 text-app-text rounded border px-2 py-1"
                            >
                                {QUOTE_ASSET_OPTIONS.map((opt) => (
                                    <option key={opt} value={opt}>
                                        {opt}
                                    </option>
                                ))}
                            </select>
                        </label>

                        <label className="text-app-text/75 flex flex-col gap-1">
                            Margin
                            <input
                                type="number"
                                min={0}
                                step="any"
                                value={margin}
                                onChange={(e) =>
                                    setMargin(Number(e.target.value))
                                }
                                className="border-line-muted bg-ink-80 text-app-text rounded border px-2 py-1"
                            />
                        </label>

                        <label className="text-app-text/75 flex flex-col gap-1">
                            Leverage (max 100)
                            <input
                                type="number"
                                min={1}
                                max={100}
                                step={1}
                                value={lev}
                                onChange={(e) => setLev(Number(e.target.value))}
                                className="border-line-muted bg-ink-80 text-app-text rounded border px-2 py-1"
                            />
                        </label>

                        <label className="text-app-text/75 flex flex-col gap-1">
                            Warmup Candles
                            <input
                                type="number"
                                min={0}
                                step={100}
                                value={warmupCandles}
                                onChange={(e) =>
                                    setWarmupCandles(Number(e.target.value))
                                }
                                className="border-line-muted bg-ink-80 text-app-text rounded border px-2 py-1"
                            />
                        </label>

                        <label className="text-app-text/75 flex flex-col gap-1">
                            Taker Fee (bps)
                            <input
                                type="number"
                                min={0}
                                step={1}
                                value={takerFeeBps}
                                onChange={(e) =>
                                    setTakerFeeBps(Number(e.target.value))
                                }
                                className="border-line-muted bg-ink-80 text-app-text rounded border px-2 py-1"
                            />
                        </label>

                        <label className="text-app-text/75 flex flex-col gap-1">
                            Maker Fee (bps)
                            <input
                                type="number"
                                min={0}
                                step={1}
                                value={makerFeeBps}
                                onChange={(e) =>
                                    setMakerFeeBps(Number(e.target.value))
                                }
                                className="border-line-muted bg-ink-80 text-app-text rounded border px-2 py-1"
                            />
                        </label>

                        <label className="text-app-text/75 flex flex-col gap-1">
                            Funding bps / 8h
                            <input
                                type="number"
                                step="any"
                                value={fundingRateBpsPer8h}
                                onChange={(e) =>
                                    setFundingRateBpsPer8h(
                                        Number(e.target.value)
                                    )
                                }
                                className="border-line-muted bg-ink-80 text-app-text rounded border px-2 py-1"
                            />
                        </label>
                    </div>

                    <div className="flex flex-col gap-2 p-2 text-sm md:flex-row md:items-center md:justify-between">
                        <div className="text-app-text/70">
                            Range:{" "}
                            <span className="text-app-text font-medium">
                                {backtestWindowLabel}
                            </span>
                            {intervalOn && (
                                <span className="text-app-text/60 ml-2">
                                    (from interval window)
                                </span>
                            )}
                        </div>

                        <button
                            onClick={() => void runBacktest()}
                            disabled={!canRunBacktest}
                            className={`rounded border px-4 py-1.5 text-sm font-semibold transition ${
                                canRunBacktest
                                    ? "border-accent-brand-strong text-accent-brand hover:bg-accent-brand-strong/20"
                                    : "border-line-weak text-app-text/35 cursor-not-allowed"
                            }`}
                        >
                            {isSubmittingBacktest
                                ? "Running..."
                                : "Run Backtest"}
                        </button>
                    </div>

                    {!selectedBacktestExchange && (
                        <p className="text-accent-danger-soft px-2 text-xs">
                            Selected exchange is chart-only for now. Backtesting
                            supports Binance, Bybit, MEXC, HTX, Coinbase.
                        </p>
                    )}

                    {backtestError && (
                        <p className="text-accent-danger-soft px-2 text-xs">
                            {backtestError}
                        </p>
                    )}
                </div>

                {/* CHART (middle) */}
                <div className="border-line-weak bg-glow-10 mb-30 flex min-h-[80vh] w-[90%] flex-1 flex-col overflow-hidden rounded-lg border-2 p-4 tracking-widest">
                    {/* Toggle + Dates */}
                    <div className="flex flex-wrap items-center gap-4 p-4 pl-1">
                        {/* Toggle Button */}
                        <button
                            onClick={() => setIntervalOn(!intervalOn)}
                            className={`relative mr-3 flex h-6 w-12 cursor-pointer items-center rounded-full transition-colors duration-300 ${
                                intervalOn ? "bg-toggle-on" : "bg-toggle-off"
                            }`}
                        >
                            <span
                                className={`bg-toggle-knob absolute top-1 left-1 h-4 w-4 rounded-full transition-transform duration-300 ${
                                    intervalOn
                                        ? "translate-x-6"
                                        : "translate-x-0"
                                }`}
                            />
                        </button>

                        <h3 className="tracking-wide">
                            Select BT period {intervalOn ? "On" : "Off"}
                        </h3>

                        <div className="flex flex-wrap items-center gap-2">
                            {RANGE_PRESETS.map((preset) => (
                                <button
                                    key={preset.id}
                                    className={`rounded border px-3 py-1 text-sm transition ${
                                        rangePreset === preset.id
                                            ? "border-accent-brand-strong text-accent-brand"
                                            : "border-line-muted text-app-text/70 hover:border-line-strong"
                                    }`}
                                    onClick={() => {
                                        handlePresetSelect(preset.id);
                                        setShowDatePicker(true);
                                    }}
                                >
                                    {preset.label}
                                </button>
                            ))}
                        </div>

                        {rangePreset === "CUSTOM" && showDatePicker && (
                            <div className="border-line-muted bg-ink-50 text-app-text flex flex-col gap-3 rounded border p-3 text-sm">
                                {customRows.map((item) => (
                                    <div
                                        key={item.label}
                                        className="flex flex-wrap items-center gap-2"
                                    >
                                        <span className="text-app-text/60 w-14 text-xs tracking-wide uppercase">
                                            {item.label}
                                        </span>
                                        <select
                                            value={item.parts.year}
                                            onChange={(e) =>
                                                item.update({
                                                    year: Number(
                                                        e.target.value
                                                    ),
                                                })
                                            }
                                            className="border-line-muted bg-ink-70 rounded border p-1"
                                        >
                                            {YEARS.map((year) => (
                                                <option key={year} value={year}>
                                                    {year}
                                                </option>
                                            ))}
                                        </select>
                                        <select
                                            value={item.parts.month}
                                            onChange={(e) =>
                                                item.update({
                                                    month: Number(
                                                        e.target.value
                                                    ),
                                                })
                                            }
                                            className="border-line-muted bg-ink-70 rounded border p-1"
                                        >
                                            {MONTHS.map((month) => (
                                                <option
                                                    key={month.value}
                                                    value={month.value}
                                                >
                                                    {month.label}
                                                </option>
                                            ))}
                                        </select>
                                        <select
                                            value={item.parts.day}
                                            onChange={(e) =>
                                                item.update({
                                                    day: Number(e.target.value),
                                                })
                                            }
                                            className="border-line-muted bg-ink-70 rounded border p-1"
                                        >
                                            {Array.from(
                                                { length: item.dayCount },
                                                (_, idx) => idx + 1
                                            ).map((day) => (
                                                <option key={day} value={day}>
                                                    {day}
                                                </option>
                                            ))}
                                        </select>
                                        <input
                                            type="time"
                                            value={item.parts.time}
                                            onChange={(e) =>
                                                item.update({
                                                    time: e.target.value,
                                                })
                                            }
                                            className="border-line-muted bg-surface-input-muted w-24 w-[115px] rounded border p-1"
                                        />
                                        <span className="text-app-text/50 text-xs">
                                            UTC
                                        </span>
                                    </div>
                                ))}

                                <button
                                    onClick={() => {
                                        confirmCustomRange();
                                        setShowDatePicker(false);
                                    }}
                                    disabled={!isCustomDirty}
                                    className={`self-start rounded border px-3 py-1 text-xs font-semibold transition ${
                                        isCustomDirty
                                            ? "border-accent-brand-strong text-accent-brand hover:bg-accent-brand-strong/20"
                                            : "border-line-weak text-app-text/30 cursor-not-allowed"
                                    }`}
                                >
                                    OK
                                </button>
                            </div>
                        )}
                        <div className="ml-auto flex flex-wrap items-center gap-2">
                            <select
                                value={selectedExchange}
                                onChange={(e) =>
                                    setSelectedExchange(
                                        e.target.value as ExchangeId
                                    )
                                }
                                className="border-line-muted bg-ink-80 text-app-text/80 rounded border px-2 py-1 text-sm"
                            >
                                {EXCHANGE_OPTIONS.map((opt) => (
                                    <option key={opt.value} value={opt.value}>
                                        {opt.label}
                                    </option>
                                ))}
                            </select>

                            <select
                                value={selectedMarket}
                                onChange={(e) =>
                                    setSelectedMarket(
                                        e.target.value as MarketType
                                    )
                                }
                                className="border-line-muted bg-ink-80 text-app-text/80 rounded border px-2 py-1 text-sm"
                            >
                                {MARKET_OPTIONS.filter((opt) =>
                                    selectedExchangeMarkets.includes(opt.value)
                                ).map((opt) => (
                                    <option key={opt.value} value={opt.value}>
                                        {opt.label}
                                    </option>
                                ))}
                            </select>

                            <select
                                value={activeAsset}
                                onChange={(e) =>
                                    nav(
                                        `/backtest/${sanitizeAsset(e.target.value)}`
                                    )
                                }
                                required
                                className="border-accent-brand-strong bg-ink-80 text-accent-brand rounded border px-3 py-1 text-sm font-semibold transition"
                            >
                                {universe.map((u) => (
                                    <option
                                        key={u.name}
                                        value={sanitizeAsset(u.name)}
                                    >
                                        {sanitizeAsset(u.name)}
                                    </option>
                                ))}
                            </select>
                        </div>
                    </div>

                    {/* Asset Title */}
                    <h2 className="bg-ink-80 rounded-t-lg p-2 text-center text-2xl font-semibold">
                        <AssetIcon
                            symbol={sanitizeAsset(activeAsset)}
                            className="mr-2 mb-1 inline-block"
                        />
                        {activeAsset}
                    </h2>

                    {/* TF SELECTOR */}
                    <div className="bg-app-surface-5 flex min-h-0 flex-[3] flex-col">
                        <div className="bg-ink-70 z-5 grid w-full grid-cols-13 text-center tracking-normal">
                            {Object.entries(TIMEFRAME_CAMELCASE).map(
                                ([short, tf]) => {
                                    const supported =
                                        supportedTimeframes.includes(tf);
                                    return (
                                        <div
                                            className={`py-2 ${
                                                supported
                                                    ? "text-app-text/70 hover:bg-ink-hover cursor-pointer"
                                                    : "text-app-text/30 cursor-not-allowed"
                                            }`}
                                            key={short}
                                            onClick={() => {
                                                if (!supported) return;
                                                setTimeframe(tf);
                                            }}
                                            title={
                                                supported
                                                    ? undefined
                                                    : "Not supported by selected source"
                                            }
                                        >
                                            <span
                                                className={`px-2 text-center text-sm ${
                                                    timeframe === tf
                                                        ? "text-timeframe-active font-bold"
                                                        : ""
                                                }`}
                                            >
                                                {short}
                                            </span>
                                        </div>
                                    );
                                }
                            )}
                        </div>

                        {/* CHART PROVIDER + CHART */}
                        <ChartContainer
                            asset={activeAsset}
                            tf={timeframe}
                            settingInterval={intervalOn}
                            candleData={candleData}
                        />
                    </div>
                    {/* CONSOLE*/}
                    <div className="mt-4 flex min-h-0 flex-[2] flex-col p-2 text-xl font-semibold tracking-wide">
                        <h2 className="p-2 text-center text-2xl font-semibold">
                            Result
                        </h2>
                        {resultViewState === "loading" && (
                            <div className="border-line-muted bg-ink-80 mx-auto mt-2 flex w-full flex-1 flex-col items-center justify-center rounded border p-6 text-sm">
                                {activeRunId && (
                                    <p className="text-app-text/70 mb-2 text-center font-mono text-xs">
                                        Run ID: {activeRunId}
                                    </p>
                                )}
                                <p className="text-app-text/90 text-center text-2xl font-bold">
                                    {progressLabel ?? "Loading backtest..."}
                                </p>
                            </div>
                        )}

                        {resultViewState === "nothing" && (
                            <div className="border-line-muted bg-ink-80 mx-auto mt-2 flex w-full flex-1 items-center justify-center rounded border p-4 text-center text-sm">
                                <p className="text-app-text/55">
                                    No backtest result yet.
                                </p>
                            </div>
                        )}

                        {resultViewState === "result" && (
                            <div className="border-line-muted bg-ink-80 mx-auto mt-2 flex min-h-0 w-full flex-1 flex-col rounded border p-4 text-sm">
                                {activeRunId && (
                                    <p className="text-app-text/70 font-mono text-xs">
                                        Run ID: {activeRunId}
                                    </p>
                                )}

                                {resultToRender && (
                                    <>
                                        <div className="border-line-subtle mt-3 rounded border p-3">
                                            <p className="text-app-text/50 text-xs uppercase">
                                                Run Config
                                            </p>
                                            <div className="mt-2 grid grid-cols-1 gap-2 text-xs md:grid-cols-2 lg:grid-cols-3">
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Asset
                                                    </p>
                                                    <p className="text-app-text">
                                                        {
                                                            resultToRender
                                                                .config.asset
                                                        }
                                                    </p>
                                                </div>
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Source
                                                    </p>
                                                    <p className="text-app-text">
                                                        {resultToRender.config.source.exchange.toUpperCase()}{" "}
                                                        /{" "}
                                                        {resultToRender.config.source.market.toUpperCase()}{" "}
                                                        /{" "}
                                                        {
                                                            resultToRender
                                                                .config.source
                                                                .quoteAsset
                                                        }
                                                    </p>
                                                </div>
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Strategy
                                                    </p>
                                                    <p className="text-app-text">
                                                        {
                                                            resultToRender
                                                                .config
                                                                .strategyId
                                                        }
                                                    </p>
                                                </div>
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Resolution
                                                    </p>
                                                    <p className="text-app-text">
                                                        {fromTimeFrame(
                                                            resultToRender
                                                                .config
                                                                .resolution
                                                        )}
                                                    </p>
                                                </div>
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Window (UTC)
                                                    </p>
                                                    <p className="text-app-text">
                                                        {formatUtcMinute(
                                                            resultToRender
                                                                .config
                                                                .startTime
                                                        )}{" "}
                                                        -{" "}
                                                        {formatUtcMinute(
                                                            resultToRender
                                                                .config.endTime
                                                        )}
                                                    </p>
                                                </div>
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Margin / Leverage
                                                    </p>
                                                    <p className="text-app-text">
                                                        {num(
                                                            resultToRender
                                                                .config.margin,
                                                            2
                                                        )}{" "}
                                                        /{" "}
                                                        {
                                                            resultToRender
                                                                .config.lev
                                                        }
                                                        x
                                                    </p>
                                                </div>
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Fees (bps)
                                                    </p>
                                                    <p className="text-app-text">
                                                        taker{" "}
                                                        {
                                                            resultToRender
                                                                .config
                                                                .takerFeeBps
                                                        }{" "}
                                                        / maker{" "}
                                                        {
                                                            resultToRender
                                                                .config
                                                                .makerFeeBps
                                                        }
                                                    </p>
                                                </div>
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Funding (bps / 8h)
                                                    </p>
                                                    <p className="text-app-text">
                                                        {num(
                                                            resultToRender
                                                                .config
                                                                .fundingRateBpsPer8h,
                                                            4
                                                        )}
                                                    </p>
                                                </div>
                                                <div>
                                                    <p className="text-app-text/50">
                                                        Snapshot Interval
                                                    </p>
                                                    <p className="text-app-text">
                                                        {
                                                            resultToRender
                                                                .config
                                                                .snapshotIntervalCandles
                                                        }{" "}
                                                        candles
                                                    </p>
                                                </div>
                                            </div>
                                        </div>

                                        <div className="mt-3 grid grid-cols-2 gap-2 md:grid-cols-4">
                                            <div className="border-line-subtle rounded border p-2">
                                                <p className="text-app-text/50 text-xs">
                                                    Trades
                                                </p>
                                                <p>
                                                    {
                                                        resultToRender.summary
                                                            .totalTrades
                                                    }
                                                </p>
                                            </div>
                                            <div className="border-line-subtle rounded border p-2">
                                                <p className="text-app-text/50 text-xs">
                                                    Net PnL
                                                </p>
                                                <p
                                                    className={
                                                        resultToRender.summary
                                                            .netPnl >= 0
                                                            ? "text-accent-success"
                                                            : "text-accent-danger-soft"
                                                    }
                                                >
                                                    {resultToRender.summary
                                                        .netPnl >= 0
                                                        ? "+"
                                                        : ""}
                                                    {num(
                                                        resultToRender.summary
                                                            .netPnl,
                                                        2
                                                    )}
                                                </p>
                                            </div>
                                            <div className="border-line-subtle rounded border p-2">
                                                <p className="text-app-text/50 text-xs">
                                                    Return %
                                                </p>
                                                <p>
                                                    {num(
                                                        resultToRender.summary
                                                            .returnPct,
                                                        2
                                                    )}
                                                    %
                                                </p>
                                            </div>
                                            <div className="border-line-subtle rounded border p-2">
                                                <p className="text-app-text/50 text-xs">
                                                    Sharpe
                                                </p>
                                                <p>
                                                    {resultToRender.summary
                                                        .sharpeRatio == null
                                                        ? "—"
                                                        : num(
                                                              resultToRender
                                                                  .summary
                                                                  .sharpeRatio,
                                                              3
                                                          )}
                                                </p>
                                            </div>
                                            <div className="border-line-subtle rounded border p-2">
                                                <p className="text-app-text/50 text-xs">
                                                    Win Rate
                                                </p>
                                                <p>
                                                    {num(
                                                        resultToRender.summary
                                                            .winRatePct,
                                                        2
                                                    )}
                                                    %
                                                </p>
                                            </div>
                                            <div className="border-line-subtle rounded border p-2">
                                                <p className="text-app-text/50 text-xs">
                                                    Profit Factor
                                                </p>
                                                <p>
                                                    {resultToRender.summary
                                                        .profitFactor == null
                                                        ? "—"
                                                        : num(
                                                              resultToRender
                                                                  .summary
                                                                  .profitFactor,
                                                              2
                                                          )}
                                                </p>
                                            </div>
                                            <div className="border-line-subtle rounded border p-2">
                                                <p className="text-app-text/50 text-xs">
                                                    Max DD %
                                                </p>
                                                <p>
                                                    {num(
                                                        resultToRender.summary
                                                            .maxDrawdownPct,
                                                        2
                                                    )}
                                                    %
                                                </p>
                                            </div>
                                            <div className="border-line-subtle rounded border p-2">
                                                <p className="text-app-text/50 text-xs">
                                                    Candles
                                                </p>
                                                <p>
                                                    {
                                                        resultToRender.candlesProcessed
                                                    }
                                                </p>
                                            </div>
                                        </div>

                                        <div className="mt-4 min-h-0 flex-1 overflow-auto">
                                            <table className="w-full min-w-[760px] text-left text-xs">
                                                <thead className="text-app-text/60 border-line-subtle border-b uppercase">
                                                    <tr>
                                                        <th className="py-2 pr-4 text-left">
                                                            Side
                                                        </th>
                                                        <th className="py-2 pr-4 text-right">
                                                            Open
                                                        </th>
                                                        <th className="py-2 pr-4 text-right">
                                                            Close
                                                        </th>
                                                        <th className="py-2 pr-4 text-right">
                                                            PnL
                                                        </th>
                                                        <th className="py-2 pr-4 text-right">
                                                            Size
                                                        </th>
                                                        <th className="py-2 pr-4 text-right">
                                                            Fee
                                                        </th>
                                                        <th className="py-2 pr-4 text-right">
                                                            Funding
                                                        </th>
                                                        <th className="py-2 text-right">
                                                            Open Time - Close
                                                            Time
                                                        </th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {resultToRender.trades
                                                        .length === 0 ? (
                                                        <tr>
                                                            <td
                                                                colSpan={8}
                                                                className="text-app-text/45 p-3 text-center"
                                                            >
                                                                No trades in
                                                                this run.
                                                            </td>
                                                        </tr>
                                                    ) : (
                                                        resultToRender.trades.map(
                                                            (trade, idx) => (
                                                                <tr
                                                                    key={`${resultToRender.runId}-${idx}`}
                                                                    className="border-line-subtle border-b last:border-b-0"
                                                                >
                                                                    <td
                                                                        className={`py-2 pr-4 font-semibold uppercase ${
                                                                            trade.side ===
                                                                            "long"
                                                                                ? "text-accent-success-strong"
                                                                                : "text-accent-danger"
                                                                        }`}
                                                                    >
                                                                        {
                                                                            trade.side
                                                                        }
                                                                    </td>
                                                                    <td className="py-2 pr-4 text-right">
                                                                        {formatPrice(
                                                                            trade
                                                                                .open
                                                                                .price
                                                                        )}
                                                                    </td>
                                                                    <td className="py-2 pr-4 text-right">
                                                                        {formatPrice(
                                                                            trade
                                                                                .close
                                                                                .price
                                                                        )}
                                                                    </td>
                                                                    <td
                                                                        className={`py-2 pr-4 text-right ${
                                                                            trade.pnl >=
                                                                            0
                                                                                ? "text-accent-success"
                                                                                : "text-accent-danger-soft"
                                                                        }`}
                                                                    >
                                                                        {num(
                                                                            trade.pnl,
                                                                            2
                                                                        )}
                                                                        $
                                                                    </td>
                                                                    <td className="py-2 pr-4 text-right">
                                                                        {num(
                                                                            trade.size,
                                                                            4
                                                                        )}
                                                                    </td>
                                                                    <td className="py-2 pr-4 text-right">
                                                                        {num(
                                                                            trade.fees,
                                                                            4
                                                                        )}
                                                                        $
                                                                    </td>
                                                                    <td className="py-2 pr-4 text-right">
                                                                        {num(
                                                                            trade.funding,
                                                                            4
                                                                        )}
                                                                    </td>
                                                                    <td className="py-2 text-right">
                                                                        {formatUTC(
                                                                            trade
                                                                                .open
                                                                                .time
                                                                        )}{" "}
                                                                        -{" "}
                                                                        {formatUTC(
                                                                            trade
                                                                                .close
                                                                                .time
                                                                        )}
                                                                    </td>
                                                                </tr>
                                                            )
                                                        )
                                                    )}
                                                </tbody>
                                            </table>
                                        </div>
                                    </>
                                )}
                            </div>
                        )}
                    </div>
                </div>
            </div>
        </div>
    );
}

// -----------------------
// Backtest Component (with ChartProvider)
// -----------------------
export default function Backtest() {
    const { asset: routeAsset } = useParams<{ asset: string }>();

    return <BacktestContent routeAsset={routeAsset} />;
}
