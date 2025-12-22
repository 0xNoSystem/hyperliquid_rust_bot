import { useState, useEffect, useMemo, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
    TIMEFRAME_CAMELCASE,
    fromTimeFrame,
    TF_TO_MS,
    sanitizeAsset,
} from "../types";
import type { TimeFrame } from "../types";
import ChartContainer from "../chart/ChartContainer";
import { getTimeframeCache } from "../chart/candleCache";
import { fetchCandles } from "../chart/utils";
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

async function loadCandles(
    tf: TimeFrame,
    startMs: number,
    endMs: number,
    asset: string,
    setCached?: (c: CandleData[]) => void
): Promise<CandleData[]> {
    if (!asset) return [];

    const candleIntervalMs = TF_TO_MS[tf];
    const prefetchBuffer = 200 * candleIntervalMs;

    let rangeStart = Math.max(0, startMs - prefetchBuffer);
    let rangeEnd = Math.min(Date.now(), endMs + prefetchBuffer);

    // Fallback to recent window if range is invalid
    if (!rangeStart || !rangeEnd || rangeEnd <= rangeStart) {
        rangeEnd = Date.now();
        rangeStart = rangeEnd - 30 * 24 * 60 * 60 * 1000;
    }

    const { normalizedStart, normalizedEnd } = normalizeRange(
        rangeStart,
        rangeEnd,
        candleIntervalMs
    );
    const expectedCandles = Math.ceil(
        (normalizedEnd - normalizedStart) / candleIntervalMs
    );
    const tfCache = getTimeframeCache(tf, asset);
    const { cached, missing } = collectCachedCandles(
        tfCache,
        asset,
        normalizedStart,
        normalizedEnd,
        candleIntervalMs
    );
    const fullCache = cacheToArray(tfCache, asset);
    if (setCached) {
        setCached(fullCache);
    }

    console.log(
        `%c[LOAD CANDLES] TF=${tf}, Expected=${expectedCandles}, Cached=${cached.length}`,
        "color: orange; font-weight: bold;"
    );

    // Serve straight from cache when we already have full coverage
    if (missing.length === 0 && cached.length > 0) {
        return fullCache;
    }

    try {
        for (const segment of missing) {
            const data = await fetchCandles(
                asset,
                segment.start,
                segment.end,
                fromTimeFrame(tf)
            );

            for (const candle of data) {
                tfCache.set(candle.start, candle);
            }
        }
    } catch (err) {
        console.error("Failed to fetch candles", err);
        return fullCache;
    }

    const merged = cacheToArray(tfCache, asset);

    return merged;
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

        const timer = setTimeout(() => {
            (async () => {
                const data = await loadCandles(
                    timeframe,
                    startTime,
                    endTime,
                    routeAsset,
                    setCandleData
                );
                setCandleData(data);
            })();
        }, 200);

        return () => clearTimeout(timer);
    }, [startTime, endTime, timeframe, intervalOn, routeAsset]);

    return (
        <div className="flex flex-1 flex-col bg-black/10 pb-50">
            {/* Title */}
            <h1 className="mt-6 p-2 text-center text-3xl font-bold tracking-widest">
                STRATEGY LAB
            </h1>

            {/* Layout */}
            <div className="z-1 flex flex-grow flex-col items-center justify-between py-8">
                {/* STRATEGY (top) */}
                <div className="mb-6 mb-30 w-[60%] border-2 border-white/70 bg-black/60 p-4 text-center tracking-widest">
                    <h2 className="p-2 text-xl font-semibold">Strategy</h2>
                </div>

                {/* CHART (middle) */}
                <div className="mb-30 flex h-[80vh] min-h-fit w-[90%] flex-grow flex-col rounded-lg border-2 border-white/20 bg-white/10 p-4 tracking-widest">
                    {/* Toggle + Dates */}
                    <div className="flex flex-wrap items-center gap-4 p-4 pl-1">
                        {/* Toggle Button */}
                        <button
                            onClick={() => setIntervalOn(!intervalOn)}
                            className={`relative mr-3 flex h-6 w-12 cursor-pointer items-center rounded-full transition-colors duration-300 ${
                                intervalOn ? "bg-orange-500" : "bg-gray-500"
                            }`}
                        >
                            <span
                                className={`absolute top-1 left-1 h-4 w-4 rounded-full bg-white transition-transform duration-300 ${
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
                                            ? "border-orange-500 text-orange-400"
                                            : "border-white/30 text-white/70 hover:border-white/60"
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
                            <div className="flex flex-col gap-3 rounded border border-white/30 bg-black/50 p-3 text-sm text-white">
                                {customRows.map((item) => (
                                    <div
                                        key={item.label}
                                        className="flex flex-wrap items-center gap-2"
                                    >
                                        <span className="w-14 text-xs tracking-wide text-white/60 uppercase">
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
                                            className="rounded border border-white/30 bg-black/70 p-1"
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
                                            className="rounded border border-white/30 bg-black/70 p-1"
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
                                            className="rounded border border-white/30 bg-black/70 p-1"
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
                                            className="w-24 w-[115px] rounded border border-white/30 bg-gray-600/70 p-1"
                                        />
                                        <span className="text-xs text-white/50">
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
                                            ? "border-orange-500 text-orange-400 hover:bg-orange-500/20"
                                            : "cursor-not-allowed border-white/20 text-white/30"
                                    }`}
                                >
                                    OK
                                </button>
                            </div>
                        )}
                        <div className="ml-auto">
                            <select
                                value={activeAsset}
                                onChange={(e) =>
                                    nav(
                                        `/backtest/${sanitizeAsset(e.target.value)}`
                                    )
                                }
                                required
                                className="rounded border border-orange-500 bg-black/80 px-3 py-1 text-sm font-semibold text-orange-400 transition"
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
                    <h2 className="rounded-t-lg bg-black/80 p-2 text-center text-2xl font-semibold">
                        <AssetIcon
                            symbol={sanitizeAsset(activeAsset)}
                            className="mr-2 mb-1 inline-block"
                        />
                        {activeAsset}
                    </h2>

                    {/* TF SELECTOR */}
                    <div className="flex flex-1 flex-col rounded-b-lg border-2 border-black/30 bg-[#111212]">
                        <div className="z-5 grid w-full grid-cols-13 bg-black/70 text-center tracking-normal">
                            {Object.entries(TIMEFRAME_CAMELCASE).map(
                                ([short, tf]) => (
                                    <div
                                        className="cursor-pointer border-b-2 border-black py-2 text-white/70 hover:bg-black"
                                        key={short}
                                        onClick={() => {
                                            setTimeframe(tf);
                                        }}
                                    >
                                        <span
                                            className={`px-2 text-center text-sm ${
                                                timeframe === tf
                                                    ? "font-bold text-orange-500"
                                                    : ""
                                            }`}
                                        >
                                            {short}
                                        </span>
                                    </div>
                                )
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
