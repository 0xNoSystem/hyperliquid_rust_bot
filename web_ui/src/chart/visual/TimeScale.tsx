import React, { useRef, useEffect } from "react";
import { useChartContext } from "../ChartContext";
import { timeToX, xToTime, formatUTC, computeTimePan } from "../utils";
import { MAX_CANDLE_WIDTH } from "../constants";
import { TF_TO_MS } from "../../types";

type TouchPoint = {
    clientX: number;
    clientY: number;
};

function computeTimeDragZoom(
    initialStart: number,
    initialEnd: number,
    totalDx: number
) {
    const initialRange = initialEnd - initialStart;
    const center = (initialStart + initialEnd) / 2;

    // Drag right → totalDx > 0 → zoom OUT
    // Drag left → totalDx < 0 → zoom IN
    const speed = 0.002;
    const factor = 1 + totalDx * speed;

    const newRange = Math.max(1, initialRange * factor);

    return {
        start: center - newRange / 2,
        end: center + newRange / 2,
    };
}

function computeTimeWheelZoom(
    startTime: number,
    endTime: number,
    deltaY: number
) {
    const range = endTime - startTime;
    const center = (startTime + endTime) / 2;

    const speed = 0.0015;
    const factor = 1 + deltaY * speed;

    const newRange = Math.max(1, range * factor);

    return {
        start: center - newRange / 2,
        end: center + newRange / 2,
    };
}

const clamp = (value: number, min: number, max: number) =>
    Math.min(Math.max(value, min), max);

const TIME_STEPS_MS = [
    60_000, // 1m
    3 * 60_000, // 3m
    5 * 60_000, // 5m
    10 * 60_000, // 10m
    15 * 60_000, // 15m
    30 * 60_000, // 30m
    60 * 60_000, // 1h
    2 * 60 * 60_000, // 2h
    4 * 60 * 60_000, // 4h
    6 * 60 * 60_000, // 6h
    12 * 60 * 60_000, // 12h
    24 * 60 * 60_000, // 1d
    2 * 24 * 60 * 60_000, // 2d
    3 * 24 * 60 * 60_000, // 3d
    7 * 24 * 60 * 60_000, // 1w
    14 * 24 * 60 * 60_000, // 2w
    30 * 24 * 60 * 60_000, // ~1M
    90 * 24 * 60 * 60_000, // ~3M
    180 * 24 * 60 * 60_000, // ~6M
    365 * 24 * 60 * 60_000, // ~1Y
];

const YEAR_MS = 365 * 24 * 60 * 60_000;
const YEAR_STEPS = [1, 2, 5, 10, 20, 50, 100];

const pickTimeStep = (minStep: number) => {
    const fromList = TIME_STEPS_MS.find((step) => step >= minStep);
    if (fromList) return fromList;
    const minYears = minStep / YEAR_MS;
    const yearStep =
        YEAR_STEPS.find((years) => years >= minYears) ??
        Math.ceil(minYears / 100) * 100;
    return yearStep * YEAR_MS;
};

const isSameDayUtc = (a: number, b: number) => {
    const da = new Date(a);
    const db = new Date(b);
    return (
        da.getUTCFullYear() === db.getUTCFullYear() &&
        da.getUTCMonth() === db.getUTCMonth() &&
        da.getUTCDate() === db.getUTCDate()
    );
};

const isSameMonthUtc = (a: number, b: number) => {
    const da = new Date(a);
    const db = new Date(b);
    return (
        da.getUTCFullYear() === db.getUTCFullYear() &&
        da.getUTCMonth() === db.getUTCMonth()
    );
};

const isYearBoundaryUtc = (t: number) => {
    const d = new Date(t);
    return d.getUTCMonth() === 0 && d.getUTCDate() === 1;
};

const formatTimeUtc = (t: number) =>
    new Date(t).toLocaleTimeString("en-US", {
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
        timeZone: "UTC",
    });

const formatMonthDayUtc = (t: number) =>
    new Date(t).toLocaleDateString("en-US", {
        month: "short",
        day: "numeric",
        timeZone: "UTC",
    });

const formatMonthUtc = (t: number) =>
    new Date(t).toLocaleDateString("en-US", {
        month: "short",
        timeZone: "UTC",
    });

const formatYearUtc = (t: number) =>
    new Date(t).toLocaleDateString("en-US", {
        year: "numeric",
        timeZone: "UTC",
    });

const formatMonthYearUtc = (t: number) =>
    new Date(t).toLocaleDateString("en-US", {
        month: "short",
        year: "numeric",
        timeZone: "UTC",
    });

const formatDateUtc = (t: number) =>
    new Date(t).toLocaleDateString("en-US", {
        month: "short",
        day: "numeric",
        year: "numeric",
        timeZone: "UTC",
    });

const formatDateTimeUtc = (t: number) =>
    new Date(t).toLocaleString("en-US", {
        month: "short",
        day: "numeric",
        year: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
        timeZone: "UTC",
    });

const addMonthsUtc = (date: Date, delta: number) => {
    const year = date.getUTCFullYear();
    const month = date.getUTCMonth();
    return new Date(Date.UTC(year, month + delta, 1));
};

const resolveMonthStep = (stepMs: number) => {
    if (stepMs < 60 * 24 * 60 * 60_000) return 1;
    if (stepMs < 120 * 24 * 60 * 60_000) return 3;
    if (stepMs < 240 * 24 * 60 * 60_000) return 6;
    return 12;
};

const resolveYearStep = (stepMs: number) =>
    Math.max(1, Math.round(stepMs / YEAR_MS));

const buildTicks = (
    stepMs: number,
    startTime: number,
    endTime: number,
    width: number
) => {
    const ticks: { t: number; x: number }[] = [];
    if (stepMs <= 0 || endTime <= startTime || width <= 0) return ticks;
    const endBuffer = endTime + stepMs;

    if (stepMs >= YEAR_MS) {
        const yearStep = resolveYearStep(stepMs);
        const startDate = new Date(startTime);
        const baseYear = startDate.getUTCFullYear();
        const alignedYear = Math.floor(baseYear / yearStep) * yearStep;
        let cursor = new Date(Date.UTC(alignedYear, 0, 1));
        if (cursor.getTime() > startTime) {
            cursor = new Date(Date.UTC(alignedYear - yearStep, 0, 1));
        }
        while (cursor.getTime() <= endBuffer) {
            const t = cursor.getTime();
            const x = timeToX(t, startTime, endTime, width);
            if (x >= -5 && x <= width + 5) {
                ticks.push({ t, x });
                if (ticks.length > 400) break;
            }
            cursor = new Date(
                Date.UTC(cursor.getUTCFullYear() + yearStep, 0, 1)
            );
        }
        return ticks;
    }

    if (stepMs >= 30 * 24 * 60 * 60_000) {
        const monthStep = resolveMonthStep(stepMs);
        const startDate = new Date(startTime);
        const baseYear = startDate.getUTCFullYear();
        const baseMonth = startDate.getUTCMonth();
        const alignedMonth = Math.floor(baseMonth / monthStep) * monthStep;
        let cursor = new Date(Date.UTC(baseYear, alignedMonth, 1));
        if (cursor.getTime() > startTime) {
            cursor = addMonthsUtc(cursor, -monthStep);
        }
        while (cursor.getTime() <= endBuffer) {
            const t = cursor.getTime();
            const x = timeToX(t, startTime, endTime, width);
            if (x >= -5 && x <= width + 5) {
                ticks.push({ t, x });
                if (ticks.length > 400) break;
            }
            cursor = addMonthsUtc(cursor, monthStep);
        }
        return ticks;
    }

    const firstTick = Math.floor(startTime / stepMs) * stepMs;
    for (let t = firstTick; t <= endBuffer; t += stepMs) {
        const x = timeToX(t, startTime, endTime, width);
        if (x < -5 || x > width + 5) continue;
        ticks.push({ t, x });
        if (ticks.length > 400) break;
    }

    return ticks;
};

const alignToPeriodStart = (t: number, periodMs: number) => {
    if (!Number.isFinite(periodMs) || periodMs <= 0) return t;
    return Math.floor((t - periodMs / 2) / periodMs) * periodMs;
};

const TimeScale: React.FC = () => {
    const {
        width,
        height,
        startTime,
        endTime,
        crosshairX,
        mouseOnChart,
        setTimeRange,
        selectingInterval,
        timeframe,
        intervalStartX,
        intervalEndX,
    } = useChartContext();

    const minRange = timeframe ? (TF_TO_MS[timeframe] ?? 1) : 1;
    const candleDurationMs = timeframe ? (TF_TO_MS[timeframe] ?? 0) : 0;
    const minRangeForMaxWidth =
        width > 0 && candleDurationMs > 0
            ? (candleDurationMs * width) / MAX_CANDLE_WIDTH
            : 0;
    const minZoomRange = Math.max(minRange, minRangeForMaxWidth);
    const touchState = useRef<{
        mode: "drag" | "pinch";
        startX?: number;
        startDistance?: number;
        initialStart: number;
        initialEnd: number;
        anchorRatio?: number;
    } | null>(null);

    const ref = useRef<SVGSVGElement>(null);

    const range = endTime - startTime;
    const labelMinPx = 110;
    const minLabelStepFromPx =
        range > 0 && width > 0 ? (range * labelMinPx) / width : 0;
    const minLabelStep = Math.max(minLabelStepFromPx, candleDurationMs || 1);
    const labelStep = pickTimeStep(minLabelStep);
    const showDateOnFirst = range >= 24 * 60 * 60_000;

    const labelTicks = buildTicks(labelStep, startTime, endTime, width);

    const getTickLabel = (t: number, prev: number | null, stepMs: number) => {
        if (stepMs < 24 * 60 * 60_000) {
            const showDate =
                (prev === null && showDateOnFirst) ||
                (prev !== null && !isSameDayUtc(prev, t));
            return {
                label: showDate ? formatMonthDayUtc(t) : formatTimeUtc(t),
                major: showDate,
            };
        }

        if (stepMs < 30 * 24 * 60 * 60_000) {
            const monthChanged = prev !== null && !isSameMonthUtc(prev, t);
            return {
                label: monthChanged ? formatMonthUtc(t) : formatMonthDayUtc(t),
                major: monthChanged,
            };
        }

        if (stepMs < 365 * 24 * 60 * 60_000) {
            const yearBoundary = isYearBoundaryUtc(t);
            return {
                label: yearBoundary ? formatYearUtc(t) : formatMonthUtc(t),
                major: yearBoundary,
            };
        }

        return { label: formatYearUtc(t), major: isYearBoundaryUtc(t) };
    };

    const crosshairTime =
        crosshairX !== null
            ? xToTime(crosshairX, startTime, endTime, width)
            : null;
    const crosshairXValue = crosshairX ?? 0;
    const formatCrosshairTime = (t: number) => {
        if (!timeframe) return formatUTC(t);
        if (timeframe === "month") {
            return formatMonthYearUtc(t);
        }
        if (timeframe === "week") {
            const aligned = alignToPeriodStart(t, 7 * 24 * 60 * 60_000);
            return formatDateUtc(aligned);
        }
        if (timeframe === "day1" || timeframe === "day3") {
            const aligned = alignToPeriodStart(t, TF_TO_MS[timeframe] ?? 0);
            return formatDateUtc(aligned);
        }
        const tfMs = TF_TO_MS[timeframe] ?? 0;
        if (tfMs > 0) {
            const aligned = alignToPeriodStart(t, tfMs);
            return formatDateTimeUtc(aligned);
        }
        return formatUTC(t);
    };

    const labelTicksWithInfo = labelTicks.map((tick, idx) => {
        const prev = idx > 0 ? labelTicks[idx - 1].t : null;
        const labelInfo = getTickLabel(tick.t, prev, labelStep);
        return {
            ...tick,
            label: labelInfo.label,
            major: labelInfo.major,
        };
    });

    const beginTouchDrag = (touch: TouchPoint) => {
        touchState.current = {
            mode: "drag",
            startX: touch.clientX,
            initialStart: startTime,
            initialEnd: endTime,
        };
    };

    const beginTouchPinch = (t1: TouchPoint, t2: TouchPoint) => {
        const distance = Math.hypot(
            t2.clientX - t1.clientX,
            t2.clientY - t1.clientY
        );
        const rect = ref.current?.getBoundingClientRect();
        const midX =
            rect && width > 0
                ? clamp((t1.clientX + t2.clientX) / 2 - rect.left, 0, width)
                : width / 2;

        touchState.current = {
            mode: "pinch",
            startDistance: Math.max(1, distance),
            initialStart: startTime,
            initialEnd: endTime,
            anchorRatio: width > 0 ? midX / width : 0.5,
        };
    };

    const onTouchStart = (e: React.TouchEvent) => {
        if (e.touches.length === 1) {
            beginTouchDrag(e.touches[0]);
        } else if (e.touches.length >= 2) {
            beginTouchPinch(e.touches[0], e.touches[1]);
        }
    };

    const onTouchMove = (e: React.TouchEvent) => {
        if (!touchState.current) return;

        if (touchState.current.mode === "drag" && e.touches.length === 1) {
            const state = touchState.current;
            const dx = e.touches[0].clientX - (state.startX ?? 0);

            const { start, end } = computeTimeDragZoom(
                state.initialStart,
                state.initialEnd,
                dx
            );

            const newRange = end - start;
            if (dx < 0 && newRange <= minZoomRange) return;

            setTimeRange(start, end);
            return;
        }

        if (touchState.current.mode === "pinch" && e.touches.length >= 2) {
            const state = touchState.current;
            const distance = Math.hypot(
                e.touches[1].clientX - e.touches[0].clientX,
                e.touches[1].clientY - e.touches[0].clientY
            );
            const initialRange = state.initialEnd - state.initialStart;
            if (!state.startDistance || initialRange <= 0) return;

            let newRange =
                initialRange * (state.startDistance / Math.max(1, distance));
            newRange = Math.max(minZoomRange, newRange);

            const anchorRatio = state.anchorRatio ?? 0.5;
            const anchorTime = state.initialStart + anchorRatio * initialRange;
            const newStart = anchorTime - anchorRatio * newRange;
            const newEnd = newStart + newRange;

            setTimeRange(newStart, newEnd);
        }
    };

    const onTouchEnd = (e: React.TouchEvent) => {
        if (e.touches.length === 1) {
            beginTouchDrag(e.touches[0]);
            return;
        }

        if (e.touches.length === 0) {
            touchState.current = null;
        }
    };

    // --- Wheel zoom / horizontal pan ---
    const onWheel = (e: React.WheelEvent) => {
        e.stopPropagation();

        const wantsPan = e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY);

        if (wantsPan && width > 0) {
            const horizontalDelta =
                e.shiftKey && e.deltaX === 0 ? e.deltaY : e.deltaX;

            const { start, end } = computeTimePan(
                startTime,
                endTime,
                -horizontalDelta,
                width
            );

            setTimeRange(start, end);
            return;
        }

        const { start, end } = computeTimeWheelZoom(
            startTime,
            endTime,
            e.deltaY
        );

        const newRange = end - start;
        if (e.deltaY < 0 && newRange <= minZoomRange) return;

        setTimeRange(start, end);
    };

    // --- Drag zoom (RIGHT = zoom out, LEFT = zoom in) ---
    const onMouseDown = (e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();

        const initialStart = startTime;
        const initialEnd = endTime;
        const startX = e.clientX;

        const handleMove = (ev: MouseEvent) => {
            const totalDx = ev.clientX - startX;

            const { start, end } = computeTimeDragZoom(
                initialStart,
                initialEnd,
                totalDx
            );

            const newRange = end - start;
            if (totalDx < 0 && newRange <= minZoomRange) return;

            setTimeRange(start, end);
        };

        const handleUp = () => {
            window.removeEventListener("mousemove", handleMove);
            window.removeEventListener("mouseup", handleUp);
        };

        window.addEventListener("mousemove", handleMove);
        window.addEventListener("mouseup", handleUp);
    };

    // --- Block scroll chaining completely ---
    useEffect(() => {
        const node = ref.current;
        if (!node) return;

        const blockScroll = (e: WheelEvent) => e.preventDefault();
        node.addEventListener("wheel", blockScroll, { passive: false });

        return () => node.removeEventListener("wheel", blockScroll);
    }, []);

    useEffect(() => {
        const node = ref.current;
        if (!node) return;

        const blockTouch = (e: TouchEvent) => e.preventDefault();
        node.addEventListener("touchstart", blockTouch, { passive: false });
        node.addEventListener("touchmove", blockTouch, { passive: false });

        return () => {
            node.removeEventListener("touchstart", blockTouch);
            node.removeEventListener("touchmove", blockTouch);
        };
    }, []);

    const fontSize = Math.max(10, Math.min(14, height * 0.06));

    return (
        <div
            className="pb-2"
            style={{ touchAction: "none", overscrollBehavior: "contain" }}
            onWheel={onWheel}
            onMouseDown={onMouseDown}
            onTouchStart={onTouchStart}
            onTouchMove={onTouchMove}
            onTouchEnd={onTouchEnd}
            onTouchCancel={onTouchEnd}
        >
            <svg
                ref={ref}
                width={width}
                height={25}
                style={{ overflow: "visible", overscrollBehavior: "none" }}
            >
                {/* Tick Lines + Labels */}
                {labelTicksWithInfo.map((tick, idx) => {
                    const isLast = idx === labelTicksWithInfo.length - 1;
                    const hasLabel = tick.label !== undefined;
                    const isMajor = Boolean(hasLabel && tick.major);
                    const lineOpacity = isMajor ? 0.6 : 0.35;
                    const lineWidth = isMajor ? 1 : 0.8;
                    const hideLine = isLast && !hasLabel;
                    return (
                        <g key={`tick-${tick.t}`}>
                            {!hideLine && (
                                <line
                                    x1={tick.x}
                                    y1={0}
                                    x2={tick.x}
                                    y2={-height - 10}
                                    stroke="#444"
                                    strokeOpacity={lineOpacity}
                                    strokeWidth={lineWidth}
                                />
                            )}
                            {hasLabel && (
                                <text
                                    x={tick.x}
                                    y={20}
                                    textAnchor="middle"
                                    fill={isMajor ? "#ddd" : "#aaa"}
                                    fontSize={fontSize}
                                >
                                    {tick.label}
                                </text>
                            )}
                        </g>
                    );
                })}

                {/* Crosshair Time Label */}
                {crosshairX !== null &&
                    crosshairTime !== null &&
                    mouseOnChart &&
                    !selectingInterval && (
                        <>
                            <rect
                                x={crosshairXValue - 70}
                                y={0}
                                width={160}
                                height={24}
                                fill="#2a2a2a"
                                stroke="#ffffff44"
                                strokeWidth={1}
                                rx={4}
                            />
                            <text
                                x={crosshairXValue + 10}
                                y={15}
                                textAnchor="middle"
                                fill="white"
                                fontSize={fontSize}
                                fontWeight="bold"
                            >
                                {formatCrosshairTime(crosshairTime)}
                            </text>
                        </>
                    )}

                {selectingInterval &&
                    intervalStartX !== null &&
                    intervalEndX !== null && (
                        <>
                            {[
                                { x: intervalStartX, label: "Start" },
                                { x: intervalEndX, label: "End" },
                            ].map((item, idx) => {
                                const px = clamp(
                                    timeToX(item.x, startTime, endTime, width),
                                    40,
                                    width - 40
                                );
                                const text = formatUTC(item.x);

                                return (
                                    <g key={item.label + idx}>
                                        <rect
                                            x={px - 60}
                                            y={0}
                                            width={120}
                                            height={22}
                                            fill="#151515"
                                            stroke="#ff7a18"
                                            strokeWidth={1}
                                            rx={4}
                                        />
                                        <text
                                            x={px}
                                            y={13}
                                            textAnchor="middle"
                                            fill="#ffb46a"
                                            fontSize={11}
                                            fontWeight="bold"
                                        >
                                            {text}
                                        </text>
                                    </g>
                                );
                            })}
                        </>
                    )}
            </svg>
        </div>
    );
};

export default TimeScale;
