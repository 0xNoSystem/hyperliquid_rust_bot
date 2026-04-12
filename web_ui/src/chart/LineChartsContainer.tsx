import React, {
    useRef,
    useEffect,
    useLayoutEffect,
    useState,
    useMemo,
    useCallback,
} from "react";
import type { ReactNode } from "react";
import { LineContainerCtx } from "./LineContainerCtx";
import { timeToX, xToTime, formatUTC, computeTimePan } from "./utils";

// ── Time axis tick helpers (ported from TimeScale) ─────────────────────────

const TIME_STEPS_MS = [
    60_000,
    3 * 60_000,
    5 * 60_000,
    10 * 60_000,
    15 * 60_000,
    30 * 60_000,
    60 * 60_000,
    2 * 60 * 60_000,
    4 * 60 * 60_000,
    6 * 60 * 60_000,
    12 * 60 * 60_000,
    24 * 60 * 60_000,
    2 * 24 * 60 * 60_000,
    3 * 24 * 60 * 60_000,
    7 * 24 * 60 * 60_000,
    14 * 24 * 60 * 60_000,
    30 * 24 * 60 * 60_000,
    90 * 24 * 60 * 60_000,
    180 * 24 * 60 * 60_000,
    365 * 24 * 60 * 60_000,
];

const YEAR_MS = 365 * 24 * 60 * 60_000;
const YEAR_STEPS = [1, 2, 5, 10, 20, 50, 100];

const pickTimeStep = (minStep: number) => {
    const fromList = TIME_STEPS_MS.find((s) => s >= minStep);
    if (fromList) return fromList;
    const minYears = minStep / YEAR_MS;
    const yearStep =
        YEAR_STEPS.find((y) => y >= minYears) ??
        Math.ceil(minYears / 100) * 100;
    return yearStep * YEAR_MS;
};

const addMonthsUtc = (date: Date, delta: number) =>
    new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + delta, 1));

const resolveMonthStep = (stepMs: number) => {
    if (stepMs < 60 * 24 * 60 * 60_000) return 1;
    if (stepMs < 120 * 24 * 60 * 60_000) return 3;
    if (stepMs < 240 * 24 * 60 * 60_000) return 6;
    return 12;
};

const resolveYearStep = (stepMs: number) =>
    Math.max(1, Math.round(stepMs / YEAR_MS));

function buildTimeTicks(
    stepMs: number,
    startTime: number,
    endTime: number,
    width: number
) {
    const ticks: { t: number; x: number }[] = [];
    if (stepMs <= 0 || endTime <= startTime || width <= 0) return ticks;
    const endBuf = endTime + stepMs;

    if (stepMs >= YEAR_MS) {
        const yearStep = resolveYearStep(stepMs);
        const baseYear = new Date(startTime).getUTCFullYear();
        const aligned = Math.floor(baseYear / yearStep) * yearStep;
        let cursor = new Date(Date.UTC(aligned, 0, 1));
        if (cursor.getTime() > startTime)
            cursor = new Date(Date.UTC(aligned - yearStep, 0, 1));
        while (cursor.getTime() <= endBuf) {
            const t = cursor.getTime();
            const x = timeToX(t, startTime, endTime, width);
            if (x >= -5 && x <= width + 5) ticks.push({ t, x });
            if (ticks.length > 400) break;
            cursor = new Date(
                Date.UTC(cursor.getUTCFullYear() + yearStep, 0, 1)
            );
        }
        return ticks;
    }

    if (stepMs >= 30 * 24 * 60 * 60_000) {
        const monthStep = resolveMonthStep(stepMs);
        const sd = new Date(startTime);
        const aligned = Math.floor(sd.getUTCMonth() / monthStep) * monthStep;
        let cursor = new Date(Date.UTC(sd.getUTCFullYear(), aligned, 1));
        if (cursor.getTime() > startTime)
            cursor = addMonthsUtc(cursor, -monthStep);
        while (cursor.getTime() <= endBuf) {
            const t = cursor.getTime();
            const x = timeToX(t, startTime, endTime, width);
            if (x >= -5 && x <= width + 5) ticks.push({ t, x });
            if (ticks.length > 400) break;
            cursor = addMonthsUtc(cursor, monthStep);
        }
        return ticks;
    }

    const firstTick = Math.floor(startTime / stepMs) * stepMs;
    for (let t = firstTick; t <= endBuf; t += stepMs) {
        const x = timeToX(t, startTime, endTime, width);
        if (x < -5 || x > width + 5) continue;
        ticks.push({ t, x });
        if (ticks.length > 400) break;
    }
    return ticks;
}

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

const formatYearUtc = (t: number) =>
    new Date(t).toLocaleDateString("en-US", {
        year: "numeric",
        timeZone: "UTC",
    });

const formatMonthUtc = (t: number) =>
    new Date(t).toLocaleDateString("en-US", {
        month: "short",
        timeZone: "UTC",
    });

function getTickLabel(
    t: number,
    prev: number | null,
    stepMs: number,
    rangeMs: number
) {
    if (stepMs < 24 * 60 * 60_000) {
        const showDate =
            (prev === null && rangeMs >= 24 * 60 * 60_000) ||
            (prev !== null &&
                new Date(prev).getUTCDate() !== new Date(t).getUTCDate());
        return showDate ? formatMonthDayUtc(t) : formatTimeUtc(t);
    }
    if (stepMs < 30 * 24 * 60 * 60_000) return formatMonthDayUtc(t);
    if (stepMs < 365 * 24 * 60 * 60_000) return formatMonthUtc(t);
    return formatYearUtc(t);
}

// ── Constants ──────────────────────────────────────────────────────────────

const VALUE_SCALE_WIDTH = 72; // must match LineChart
const TIME_SCALE_HEIGHT = 28;

// ── Props ──────────────────────────────────────────────────────────────────

export interface LineChartsContainerProps {
    startTime: number;
    endTime: number;
    onTimeRangeChange: (start: number, end: number) => void;
    children: (ctx: {
        chartWidth: number;
        crosshairX: number | null;
        panelMouseY: number | null;
    }) => ReactNode;
    className?: string;
    /** Global time extent of all data; enables pan/zoom clamping. */
    dataBounds?: { min: number; max: number };
}

// ────────────────────────────────────────────────────────────────────────────

const LineChartsContainer: React.FC<LineChartsContainerProps> = ({
    startTime,
    endTime,
    onTimeRangeChange,
    children,
    className,
    dataBounds,
}) => {
    const containerRef = useRef<HTMLDivElement>(null);
    const panelsRef = useRef<HTMLDivElement>(null);
    const [totalWidth, setTotalWidth] = useState(0);
    const [panelsHeight, setPanelsHeight] = useState(0);
    const wheelBusy = useRef(false);
    const panelBoundsRef = useRef<{ top: number; bottom: number }[]>([]);

    const [mouseX, setMouseX] = useState<number | null>(null);
    const [mouseY, setMouseY] = useState<number | null>(null);
    const [mouseOnChart, setMouseOnChart] = useState(false);

    const chartWidth = Math.max(0, totalWidth - VALUE_SCALE_WIDTH);

    // ── Resize ─────────────────────────────────────────────────────────
    useEffect(() => {
        if (!containerRef.current) return;
        const obs = new ResizeObserver(([entry]) => {
            setTotalWidth(entry.contentRect.width);
        });
        obs.observe(containerRef.current);
        return () => obs.disconnect();
    }, []);

    useEffect(() => {
        if (!panelsRef.current) return;
        const obs = new ResizeObserver(([entry]) => {
            setPanelsHeight(entry.contentRect.height);
        });
        obs.observe(panelsRef.current);
        return () => obs.disconnect();
    }, []);

    // ── Measure panel bounds for per-panel horizontal crosshair ────
    useLayoutEffect(() => {
        if (!panelsRef.current) return;
        const bounds: { top: number; bottom: number }[] = [];
        for (const child of Array.from(panelsRef.current.children)) {
            const el = child as HTMLElement;
            if (el.tagName.toLowerCase() === "svg" || el.offsetHeight < 50)
                continue;
            bounds.push({
                top: el.offsetTop,
                bottom: el.offsetTop + el.offsetHeight,
            });
        }
        panelBoundsRef.current = bounds;
    }, [panelsHeight]);

    // ── Context for child panels ───────────────────────────────────────
    const ctxValue = useMemo(
        () => ({ panelMouseY: mouseOnChart ? mouseY : null }),
        [mouseOnChart, mouseY]
    );

    // ── Time range helpers ─────────────────────────────────────────────
    const safeApply = useCallback(
        (start: number, end: number) => {
            if (!Number.isFinite(start) || !Number.isFinite(end)) return;
            let s = Math.min(start, end);
            let e = Math.max(start, end);
            let range = e - s;

            const MIN_RANGE = 1000;
            if (range < MIN_RANGE) {
                const mid = (s + e) / 2;
                s = mid - MIN_RANGE / 2;
                e = mid + MIN_RANGE / 2;
                range = MIN_RANGE;
            }

            if (dataBounds) {
                const dataRange = dataBounds.max - dataBounds.min;
                const pad = dataRange * 0.05;
                const lo = dataBounds.min - pad;
                const hi = dataBounds.max + pad;
                const maxRange = hi - lo;
                if (range > maxRange) {
                    const mid = (s + e) / 2;
                    s = mid - maxRange / 2;
                    e = mid + maxRange / 2;
                }
                if (s < lo) {
                    e += lo - s;
                    s = lo;
                }
                if (e > hi) {
                    s -= e - hi;
                    e = hi;
                }
            }

            onTimeRangeChange(s, e);
        },
        [onTimeRangeChange, dataBounds]
    );

    // ── Wheel zoom / pan (chart area) ──────────────────────────────────
    const onWheel = useCallback(
        (e: React.WheelEvent) => {
            e.stopPropagation();
            if (wheelBusy.current) return;
            wheelBusy.current = true;
            requestAnimationFrame(() => {
                wheelBusy.current = false;
            });

            const wantsPan =
                e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY);
            if (wantsPan && chartWidth > 0) {
                const dx = e.shiftKey && e.deltaX === 0 ? e.deltaY : e.deltaX;
                const { start, end } = computeTimePan(
                    startTime,
                    endTime,
                    -dx,
                    chartWidth
                );
                safeApply(start, end);
                return;
            }

            // Zoom around cursor position
            const range = endTime - startTime;
            const speed = 0.0015;
            const factor = 1 + e.deltaY * speed;
            const newRange = Math.max(1, range * factor);

            const rect = containerRef.current?.getBoundingClientRect();
            const cursorX = rect ? e.clientX - rect.left : chartWidth / 2;
            const ratio =
                chartWidth > 0
                    ? Math.min(Math.max(cursorX / chartWidth, 0), 1)
                    : 0.5;
            const anchor = startTime + ratio * range;
            safeApply(
                anchor - ratio * newRange,
                anchor + (1 - ratio) * newRange
            );
        },
        [startTime, endTime, chartWidth, safeApply]
    );

    // ── Drag pan (chart area) ──────────────────────────────────────────
    const onMouseDown = useCallback(
        (e: React.MouseEvent) => {
            e.preventDefault();
            e.stopPropagation();
            const initStart = startTime;
            const initEnd = endTime;
            const sx = e.clientX;

            const move = (ev: MouseEvent) => {
                const dx = ev.clientX - sx;
                if (chartWidth <= 0) return;
                const { start, end } = computeTimePan(
                    initStart,
                    initEnd,
                    dx,
                    chartWidth
                );
                safeApply(start, end);
            };
            const up = () => {
                window.removeEventListener("mousemove", move);
                window.removeEventListener("mouseup", up);
            };
            window.addEventListener("mousemove", move);
            window.addEventListener("mouseup", up);
        },
        [startTime, endTime, chartWidth, safeApply]
    );

    // ── Touch pan/pinch (chart area) ───────────────────────────────────
    const touchState = useRef<{
        mode: "pan" | "pinch";
        touchId?: number;
        startX: number;
        initialStart: number;
        initialEnd: number;
        startDistance?: number;
        anchorRatio?: number;
    } | null>(null);

    const onTouchStart = useCallback(
        (e: React.TouchEvent) => {
            e.stopPropagation();
            if (e.touches.length === 1) {
                touchState.current = {
                    mode: "pan",
                    touchId: e.touches[0].identifier,
                    startX: e.touches[0].clientX,
                    initialStart: startTime,
                    initialEnd: endTime,
                };
            } else if (e.touches.length >= 2) {
                const [t1, t2] = [e.touches[0], e.touches[1]];
                const dist = Math.abs(t2.clientX - t1.clientX);
                const rect = containerRef.current?.getBoundingClientRect();
                const cx =
                    rect && chartWidth > 0
                        ? Math.min(
                              Math.max(
                                  (t1.clientX + t2.clientX) / 2 - rect.left,
                                  0
                              ),
                              rect.width
                          )
                        : chartWidth / 2;
                touchState.current = {
                    mode: "pinch",
                    startDistance: Math.max(1, dist),
                    anchorRatio: chartWidth > 0 ? cx / chartWidth : 0.5,
                    startX: (t1.clientX + t2.clientX) / 2,
                    initialStart: startTime,
                    initialEnd: endTime,
                };
            }
        },
        [startTime, endTime, chartWidth]
    );

    const onTouchMove = useCallback(
        (e: React.TouchEvent) => {
            e.stopPropagation();
            if (!touchState.current) return;
            if (touchState.current.mode === "pan" && e.touches.length === 1) {
                const s = touchState.current;
                const touch =
                    Array.from(e.touches).find(
                        (t) => t.identifier === s.touchId
                    ) || e.touches[0];
                const dx = touch.clientX - s.startX;
                if (chartWidth > 0) {
                    const { start, end } = computeTimePan(
                        s.initialStart,
                        s.initialEnd,
                        dx,
                        chartWidth
                    );
                    safeApply(start, end);
                }
                return;
            }
            if (touchState.current.mode === "pinch" && e.touches.length >= 2) {
                const s = touchState.current;
                const [t1, t2] = [e.touches[0], e.touches[1]];
                const dist = Math.abs(t2.clientX - t1.clientX);
                const initRange = s.initialEnd - s.initialStart;
                if (!s.startDistance || initRange <= 0) return;
                const newRange = Math.max(
                    1,
                    initRange * (s.startDistance / Math.max(1, dist))
                );
                const anchor = s.anchorRatio ?? 0.5;
                const anchorTime = s.initialStart + anchor * initRange;
                safeApply(
                    anchorTime - anchor * newRange,
                    anchorTime - anchor * newRange + newRange
                );
            }
        },
        [chartWidth, safeApply]
    );

    const onTouchEnd = useCallback((e: React.TouchEvent) => {
        e.stopPropagation();
        if (e.touches.length === 0) touchState.current = null;
    }, []);

    // ── Block scroll chaining ──────────────────────────────────────────
    useEffect(() => {
        const node = containerRef.current;
        if (!node) return;
        const blockWheel = (e: WheelEvent) => e.preventDefault();
        const blockTouch = (e: TouchEvent) => e.preventDefault();
        node.addEventListener("wheel", blockWheel, { passive: false });
        node.addEventListener("touchstart", blockTouch, { passive: false });
        node.addEventListener("touchmove", blockTouch, { passive: false });
        return () => {
            node.removeEventListener("wheel", blockWheel);
            node.removeEventListener("touchstart", blockTouch);
            node.removeEventListener("touchmove", blockTouch);
        };
    }, []);

    // ── Mouse tracking ─────────────────────────────────────────────────
    const handleMouseMove = useCallback(
        (e: React.MouseEvent) => {
            const rect = containerRef.current?.getBoundingClientRect();
            if (!rect) return;
            const x = e.clientX - rect.left;
            const y = e.clientY - rect.top;
            setMouseX(Math.min(Math.max(x, 0), chartWidth));
            setMouseY(Math.min(Math.max(y, 0), panelsHeight));
            // Only "on chart" when inside the chart area (not Y scale or time scale)
            setMouseOnChart(
                x >= 0 && x < chartWidth && y >= 0 && y < panelsHeight
            );
        },
        [chartWidth, panelsHeight]
    );

    // ── Time scale: drag-to-zoom (right=out, left=in) ──────────────────
    const handleTimeScaleDrag = useCallback(
        (e: React.MouseEvent) => {
            e.preventDefault();
            e.stopPropagation();
            const initStart = startTime;
            const initEnd = endTime;
            const sx = e.clientX;

            const move = (ev: MouseEvent) => {
                const dx = ev.clientX - sx;
                const initialRange = initEnd - initStart;
                const center = (initStart + initEnd) / 2;
                const speed = 0.002;
                const factor = 1 + dx * speed;
                const newRange = Math.max(1, initialRange * factor);
                safeApply(center - newRange / 2, center + newRange / 2);
            };
            const up = () => {
                window.removeEventListener("mousemove", move);
                window.removeEventListener("mouseup", up);
            };
            window.addEventListener("mousemove", move);
            window.addEventListener("mouseup", up);
        },
        [startTime, endTime, safeApply]
    );

    const handleTimeScaleWheel = useCallback(
        (e: React.WheelEvent) => {
            e.stopPropagation();
            const range = endTime - startTime;
            const speed = 0.0015;
            const factor = 1 + e.deltaY * speed;
            const newRange = Math.max(1, range * factor);
            const center = (startTime + endTime) / 2;
            safeApply(center - newRange / 2, center + newRange / 2);
        },
        [startTime, endTime, safeApply]
    );

    // ── Time scale ticks ───────────────────────────────────────────────
    const timeRange = endTime - startTime;
    const minLabelStepPx =
        timeRange > 0 && chartWidth > 0 ? (timeRange * 110) / chartWidth : 0;
    const timeStep = pickTimeStep(Math.max(minLabelStepPx, 1));

    const timeTicks = useMemo(
        () => buildTimeTicks(timeStep, startTime, endTime, chartWidth),
        [timeStep, startTime, endTime, chartWidth]
    );

    // Crosshair time for the label
    const crosshairTime =
        mouseX !== null && chartWidth > 0
            ? xToTime(mouseX, startTime, endTime, chartWidth)
            : null;

    const crosshairX = mouseOnChart ? mouseX : null;

    return (
        <div
            ref={containerRef}
            className={`relative flex cursor-crosshair flex-col ${className ?? ""}`}
            style={{ touchAction: "none", overscrollBehavior: "contain" }}
            onWheel={onWheel}
            onMouseDown={onMouseDown}
            onMouseMove={handleMouseMove}
            onMouseLeave={() => {
                setMouseOnChart(false);
                setMouseX(null);
                setMouseY(null);
            }}
            onTouchStart={onTouchStart}
            onTouchMove={onTouchMove}
            onTouchEnd={onTouchEnd}
            onTouchCancel={onTouchEnd}
        >
            {/* Stacked panels */}
            <div ref={panelsRef} className="relative flex-1">
                <LineContainerCtx.Provider value={ctxValue}>
                    {children({
                        chartWidth,
                        crosshairX,
                        panelMouseY: mouseOnChart ? mouseY : null,
                    })}
                </LineContainerCtx.Provider>

                {/* Crosshair overlay — renders on top of all panels */}
                {crosshairX !== null && mouseY !== null && (
                    <svg
                        width={totalWidth}
                        height={panelsHeight}
                        className="pointer-events-none absolute inset-0 z-20"
                    >
                        {/* Vertical line spans full height */}
                        <line
                            x1={Math.round(crosshairX) + 0.5}
                            y1={0}
                            x2={Math.round(crosshairX) + 0.5}
                            y2={panelsHeight}
                            stroke="rgb(var(--app-text))"
                            strokeWidth={1}
                            opacity={0.4}
                            strokeDasharray="6 4"
                        />
                        {/* Horizontal line only in the panel under the cursor */}
                        {panelBoundsRef.current.some(
                            (b) => mouseY >= b.top && mouseY < b.bottom
                        ) && (
                            <line
                                x1={0}
                                y1={Math.round(mouseY) + 0.5}
                                x2={chartWidth}
                                y2={Math.round(mouseY) + 0.5}
                                stroke="rgb(var(--app-text))"
                                strokeWidth={1}
                                opacity={0.25}
                                strokeDasharray="4 4"
                            />
                        )}
                    </svg>
                )}
            </div>

            {/* Vertical time ticks through panels */}
            {panelsHeight > 0 && (
                <svg
                    width={chartWidth}
                    height={panelsHeight}
                    className="pointer-events-none absolute top-0 left-0 z-10"
                >
                    {timeTicks.map((tick, i) => (
                        <line
                            key={i}
                            x1={Math.round(tick.x) + 0.5}
                            x2={Math.round(tick.x) + 0.5}
                            y1={0}
                            y2={panelsHeight}
                            stroke="rgb(var(--app-grid))"
                            strokeOpacity={0.2}
                            strokeWidth={0.6}
                        />
                    ))}
                </svg>
            )}

            {/* Time scale (bottom) — drag = zoom, wheel = zoom */}
            <div
                style={{
                    height: TIME_SCALE_HEIGHT,
                    marginLeft: 0,
                    touchAction: "none",
                }}
                className="cursor-ew-resize"
                onMouseDown={handleTimeScaleDrag}
                onWheel={handleTimeScaleWheel}
            >
                <svg
                    width={chartWidth}
                    height={TIME_SCALE_HEIGHT}
                    style={{ overflow: "visible" }}
                >
                    {timeTicks.map((tick, idx) => {
                        const prev = idx > 0 ? timeTicks[idx - 1].t : null;
                        const lbl = getTickLabel(
                            tick.t,
                            prev,
                            timeStep,
                            timeRange
                        );
                        return (
                            <text
                                key={tick.t}
                                x={tick.x}
                                y={18}
                                textAnchor="middle"
                                fill="rgb(var(--app-text) / 0.6)"
                                fontSize={10}
                            >
                                {lbl}
                            </text>
                        );
                    })}

                    {/* Crosshair time label (clamped to edges) */}
                    {crosshairX !== null &&
                        crosshairTime !== null &&
                        (() => {
                            const bw = 130;
                            const bx = Math.max(
                                0,
                                Math.min(crosshairX - bw / 2, chartWidth - bw)
                            );
                            return (
                                <>
                                    <rect
                                        x={bx}
                                        y={2}
                                        width={bw}
                                        height={20}
                                        fill="rgb(var(--app-surface-2))"
                                        stroke="rgb(var(--line-subtle) / var(--line-subtle-alpha))"
                                        strokeWidth={1}
                                        rx={3}
                                    />
                                    <text
                                        x={bx + bw / 2}
                                        y={15}
                                        textAnchor="middle"
                                        fill="rgb(var(--app-text))"
                                        fontSize={10}
                                        fontWeight="bold"
                                    >
                                        {formatUTC(crosshairTime)}
                                    </text>
                                </>
                            );
                        })()}
                </svg>
            </div>
        </div>
    );
};

export default LineChartsContainer;
