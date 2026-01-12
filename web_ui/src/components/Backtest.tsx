import { useState, useEffect, useMemo, useCallback, useRef } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { TIMEFRAME_CAMELCASE, TF_TO_MS, sanitizeAsset } from "../types";
import type { TimeFrame } from "../types";
import ChartContainer from "../chart/ChartContainer";
import { isTimeframeSupported } from "../chart/dataSources";
import { loadCandles } from "../chart/loader";
import {
    DEFAULT_DATA_SOURCE,
    DEFAULT_QUOTE_ASSET,
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
import type { CandleData } from "../chart/utils";
import { useChartContext } from "../chart/ChartContextStore";
import { useWebSocketContext } from "../context/WebSocketContextStore";

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
    const { startTime, endTime, setTimeRange } = useChartContext();
    const { universe } = useWebSocketContext();
    const activeAsset = routeAsset ?? "";
    const defaultStartParts = useMemo(
        () => dateToParts(new Date(Date.now() - 7 * 24 * 60 * 60 * 1000)),
        []
    );
    const defaultEndParts = useMemo(() => dateToParts(new Date()), []);

    const [timeframe, setTimeframe] = useState<TimeFrame>("hour4");
    const [intervalOn, setIntervalOn] = useState(false);
    const [candleData, setCandleData] = useState<CandleData[]>([]);
    const [showDatePicker, setShowDatePicker] = useState(true);
    const [selectedExchange, setSelectedExchange] = useState<ExchangeId>(
        DEFAULT_DATA_SOURCE.exchange
    );
    const [selectedMarket, setSelectedMarket] = useState<MarketType>(
        DEFAULT_DATA_SOURCE.market
    );
    const requestIdRef = useRef(0);
    const abortControllerRef = useRef<AbortController | null>(null);

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
                        DEFAULT_QUOTE_ASSET,
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
    }, [startTime, endTime, timeframe, routeAsset, selectedDataSource]);

    return (
        <div className="bg-ink-10 flex flex-1 flex-col pb-50">
            {/* Title */}
            <h1 className="mt-6 p-2 text-center text-3xl font-bold tracking-widest">
                STRATEGY LAB
            </h1>

            {/* Layout */}
            <div className="z-1 flex flex-grow flex-col items-center justify-between py-8">
                {/* STRATEGY (top) */}
                <div className="border-line-stronger bg-ink-60 mb-6 mb-30 w-[60%] border-2 p-4 text-center tracking-widest">
                    <h2 className="p-2 text-xl font-semibold">Strategy</h2>
                </div>

                {/* CHART (middle) */}
                <div className="border-line-weak bg-glow-10 mb-30 flex h-[80vh] min-h-fit w-[90%] flex-grow flex-col rounded-lg border-2 p-4 tracking-widest">
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
                    <div className="bg-app-surface-5 flex flex-1 flex-col">
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
                    <div className="mt-20 p-2 text-xl font-semibold tracking-wide">
                        <h2 className="p-2 text-center text-2xl font-semibold">
                            Result
                        </h2>
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
