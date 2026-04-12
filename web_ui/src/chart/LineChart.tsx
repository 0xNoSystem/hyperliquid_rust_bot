import React, {
    useMemo,
    useContext,
    useRef,
    useLayoutEffect,
    useState,
    useCallback,
    useEffect,
} from "react";
import LineCanvas from "./LineCanvas";
import type { LineSeries, LinePoint } from "./LineCanvas";
import {
    priceToY,
    yToPrice,
    xToTime,
    zoomPriceRange,
    handleWheelZoom,
} from "./utils";
import { LineContainerCtx } from "./LineContainerCtx";

export type { LineSeries, LinePoint };

export interface LineChartProps {
    series: LineSeries[];
    startTime: number;
    endTime: number;
    /** Pixel X from container crosshair, null = not hovering */
    crosshairX: number | null;
    /** Chart area width in px (excluding Y scale) */
    chartWidth: number;
    height: number;
    zeroLine?: boolean;
    label?: string;
}

// ── Binary search ──────────────────────────────────────────────────────────

function lowerBound(arr: readonly LinePoint[], target: number): number {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (arr[mid].ts < target) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

function upperBound(arr: readonly LinePoint[], target: number): number {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (arr[mid].ts <= target) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

// ── Y axis helpers ─────────────────────────────────────────────────────────

const niceStep = (rawStep: number) => {
    if (!Number.isFinite(rawStep) || rawStep <= 0) return 0;
    const exp = Math.floor(Math.log10(rawStep));
    const base = 10 ** exp;
    const frac = rawStep / base;
    if (frac <= 1) return base;
    if (frac <= 2) return 2 * base;
    if (frac <= 2.5) return 2.5 * base;
    if (frac <= 5) return 5 * base;
    return 10 * base;
};

const inferDecimals = (value: number) => {
    const abs = Math.abs(value);
    if (!Number.isFinite(abs) || abs === 0) return 2;
    if (abs >= 100) return 0;
    if (abs >= 1) return 2;
    const leading = Math.max(0, -Math.floor(Math.log10(abs)));
    return Math.max(4, leading + 2);
};

const fmtVal = (v: number, d: number) => {
    if (!Number.isFinite(v)) return "—";
    return v.toLocaleString("en-US", {
        minimumFractionDigits: d,
        maximumFractionDigits: d,
    });
};

const VALUE_SCALE_WIDTH = 72;

const LineChart: React.FC<LineChartProps> = ({
    series,
    startTime,
    endTime,
    crosshairX,
    chartWidth,
    height,
    zeroLine = false,
    label,
}) => {
    // ── Active panel detection via context ──────────────────────────────
    const { panelMouseY } = useContext(LineContainerCtx);
    const wrapperRef = useRef<HTMLDivElement>(null);
    const offsetTopRef = useRef(0);

    useLayoutEffect(() => {
        if (wrapperRef.current) {
            offsetTopRef.current = wrapperRef.current.offsetTop;
        }
    });

    const isActivePanel =
        panelMouseY != null &&
        panelMouseY >= offsetTopRef.current &&
        panelMouseY < offsetTopRef.current + height;

    // Mouse Y local to this panel (for horizontal crosshair Y badge)
    const localMouseY = isActivePanel
        ? panelMouseY! - offsetTopRef.current
        : null;

    // ── Manual Y range ─────────────────────────────────────────────────
    const [manualY, setManualY] = useState(false);
    const [manualYMin, setManualYMin] = useState(0);
    const [manualYMax, setManualYMax] = useState(1);

    // ── Auto Y range from visible data ─────────────────────────────────
    const autoRange = useMemo(() => {
        let lo = Infinity;
        let hi = -Infinity;
        for (const s of series) {
            if (s.points.length === 0) continue;
            const first = Math.max(0, lowerBound(s.points, startTime) - 1);
            const last = Math.min(
                s.points.length,
                upperBound(s.points, endTime) + 1
            );
            for (let i = first; i < last; i++) {
                const v = s.points[i].value;
                if (v < lo) lo = v;
                if (v > hi) hi = v;
            }
        }
        if (!Number.isFinite(lo) || !Number.isFinite(hi) || lo >= hi) {
            const c = Number.isFinite(lo) ? lo : 0;
            return { minValue: c - 1, maxValue: c + 1 };
        }
        const range = hi - lo;
        return { minValue: lo - range * 0.05, maxValue: hi + range * 0.05 };
    }, [series, startTime, endTime]);

    const minValue = manualY ? manualYMin : autoRange.minValue;
    const maxValue = manualY ? manualYMax : autoRange.maxValue;

    // ── Y ticks ────────────────────────────────────────────────────────
    const valueRange = maxValue - minValue;
    const rawValStep =
        height > 0 ? valueRange / Math.max(2, Math.floor(height / 40)) : 0;
    const valStep = niceStep(rawValStep);
    const valDecimals = inferDecimals(
        (Math.abs(minValue) + Math.abs(maxValue)) / 2
    );

    const valueTicks = useMemo(() => {
        const ticks: { value: number; y: number }[] = [];
        if (valStep <= 0 || valueRange <= 0 || height <= 0) return ticks;
        const first = Math.floor(minValue / valStep) * valStep;
        const last = Math.ceil(maxValue / valStep) * valStep;
        for (let v = first; v <= last + valStep * 0.5; v += valStep) {
            const y = priceToY(v, minValue, maxValue, height);
            if (y < 4 || y > height - 4) continue;
            ticks.push({ value: v, y });
            if (ticks.length > 100) break;
        }
        return ticks;
    }, [minValue, maxValue, valStep, valueRange, height]);

    // ── Hover: resolve crosshairX to values per series ─────────────────
    const crosshairTime =
        crosshairX !== null && chartWidth > 0
            ? xToTime(crosshairX, startTime, endTime, chartWidth)
            : null;

    const crosshairValueY =
        crosshairX !== null && crosshairTime !== null
            ? (() => {
                  const s = series[0];
                  if (!s || s.points.length === 0) return null;
                  const idx = lowerBound(s.points, crosshairTime);
                  let best = idx;
                  if (idx > 0 && idx < s.points.length) {
                      if (
                          Math.abs(s.points[idx - 1].ts - crosshairTime) <
                          Math.abs(s.points[idx].ts - crosshairTime)
                      )
                          best = idx - 1;
                  } else if (idx >= s.points.length) {
                      best = s.points.length - 1;
                  }
                  return priceToY(
                      s.points[best].value,
                      minValue,
                      maxValue,
                      height
                  );
              })()
            : null;

    const hoverValues = useMemo(() => {
        if (crosshairTime === null) return null;
        return series.map((s) => {
            if (s.points.length === 0)
                return { label: s.label ?? "", color: s.color, value: null };
            const idx = lowerBound(s.points, crosshairTime);
            let best = idx;
            if (idx > 0 && idx < s.points.length) {
                if (
                    Math.abs(s.points[idx - 1].ts - crosshairTime) <
                    Math.abs(s.points[idx].ts - crosshairTime)
                )
                    best = idx - 1;
            } else if (idx >= s.points.length) {
                best = s.points.length - 1;
            }
            return {
                label: s.label ?? "",
                color: s.color,
                value: s.points[best].value,
            };
        });
    }, [series, crosshairTime]);

    // ── Zero line ──────────────────────────────────────────────────────
    const zeroY =
        zeroLine && minValue < 0 && maxValue > 0 && height > 0
            ? priceToY(0, minValue, maxValue, height)
            : null;

    // ── Y scale interactions ───────────────────────────────────────────
    const yScaleRef = useRef<SVGSVGElement>(null);

    useEffect(() => {
        const node = yScaleRef.current;
        if (!node) return;
        const block = (e: WheelEvent) => e.preventDefault();
        node.addEventListener("wheel", block, { passive: false });
        return () => node.removeEventListener("wheel", block);
    }, []);

    const onYScaleWheel = useCallback(
        (e: React.WheelEvent) => {
            e.stopPropagation();
            const { min, max } = handleWheelZoom(minValue, maxValue, e.deltaY);
            setManualY(true);
            setManualYMin(min);
            setManualYMax(max);
        },
        [minValue, maxValue]
    );

    const onYScaleMouseDown = useCallback(
        (e: React.MouseEvent) => {
            e.preventDefault();
            e.stopPropagation();
            const startMin = minValue;
            const startMax = maxValue;
            const startClientY = e.clientY;

            const move = (ev: MouseEvent) => {
                const dy = ev.clientY - startClientY;
                const { min, max } = zoomPriceRange(startMin, startMax, dy);
                setManualY(true);
                setManualYMin(min);
                setManualYMax(max);
            };
            const up = () => {
                window.removeEventListener("mousemove", move);
                window.removeEventListener("mouseup", up);
            };
            window.addEventListener("mousemove", move);
            window.addEventListener("mouseup", up);
        },
        [minValue, maxValue]
    );

    const onYScaleDoubleClick = useCallback(() => {
        setManualY(false);
    }, []);

    return (
        <div ref={wrapperRef} className="flex" style={{ height }}>
            {/* Chart area */}
            <div
                className="relative flex-1 overflow-hidden"
                style={{ width: chartWidth }}
            >
                {/* Legend + hover values */}
                {(label || series.length > 0) && (
                    <div className="pointer-events-none absolute top-1 left-2 z-10 flex items-center gap-3">
                        {label && (
                            <span className="text-app-text/50 text-xs">
                                {label}
                            </span>
                        )}
                        {series.map((s, idx) => (
                            <span
                                key={idx}
                                className="flex items-center gap-1 text-xs"
                            >
                                <span
                                    className="inline-block h-2 w-2 rounded-full"
                                    style={{ backgroundColor: s.color }}
                                />
                                <span className="text-app-text/40">
                                    {s.label ?? ""}
                                </span>
                                {hoverValues?.[idx]?.value != null && (
                                    <span
                                        className="font-mono"
                                        style={{ color: s.color }}
                                    >
                                        {fmtVal(
                                            hoverValues[idx].value!,
                                            valDecimals
                                        )}
                                    </span>
                                )}
                            </span>
                        ))}
                    </div>
                )}

                <LineCanvas
                    width={chartWidth}
                    height={height}
                    series={series}
                    startTime={startTime}
                    endTime={endTime}
                    minValue={minValue}
                    maxValue={maxValue}
                    className="absolute inset-0"
                />

                {/* Grid + zero line */}
                <svg
                    width={chartWidth}
                    height={height}
                    className="pointer-events-none absolute inset-0"
                >
                    {valueTicks.map((t, i) => (
                        <line
                            key={i}
                            x1={0}
                            x2={chartWidth}
                            y1={Math.round(t.y) + 0.5}
                            y2={Math.round(t.y) + 0.5}
                            stroke="rgb(var(--app-grid))"
                            strokeOpacity={0.3}
                            strokeWidth={0.6}
                        />
                    ))}
                    {zeroY !== null && (
                        <line
                            x1={0}
                            x2={chartWidth}
                            y1={Math.round(zeroY) + 0.5}
                            y2={Math.round(zeroY) + 0.5}
                            stroke="currentColor"
                            strokeOpacity={0.2}
                            strokeDasharray="4 4"
                        />
                    )}
                    {/* Dot on primary series at crosshair — only on active panel */}
                    {isActivePanel &&
                        crosshairX !== null &&
                        crosshairValueY !== null && (
                            <circle
                                cx={crosshairX}
                                cy={crosshairValueY}
                                r={3.5}
                                fill={series[0]?.color ?? "white"}
                                stroke="rgb(var(--app-surface-2))"
                                strokeWidth={1}
                                opacity={0.9}
                            />
                        )}
                </svg>
            </div>

            {/* Y scale (right) — drag = zoom, wheel = zoom, dblclick = reset */}
            <svg
                ref={yScaleRef}
                width={VALUE_SCALE_WIDTH}
                height={height}
                className="flex-shrink-0 cursor-ns-resize"
                style={{ overflow: "visible", touchAction: "none" }}
                onWheel={onYScaleWheel}
                onMouseDown={onYScaleMouseDown}
                onDoubleClick={onYScaleDoubleClick}
            >
                {valueTicks.map((t, i) => (
                    <text
                        key={i}
                        x={VALUE_SCALE_WIDTH / 2}
                        y={t.y}
                        textAnchor="middle"
                        alignmentBaseline="middle"
                        fill="rgb(var(--app-text) / 0.6)"
                        fontSize={10}
                    >
                        {fmtVal(t.value, valDecimals)}
                    </text>
                ))}
                {/* Crosshair value badge at mouse Y — like PriceScale */}
                {isActivePanel &&
                    localMouseY !== null &&
                    (() => {
                        const yVal = yToPrice(
                            localMouseY,
                            minValue,
                            maxValue,
                            height
                        );
                        return (
                            <>
                                <rect
                                    x={2}
                                    y={localMouseY - 9}
                                    width={VALUE_SCALE_WIDTH - 4}
                                    height={18}
                                    fill="rgb(var(--app-surface-2))"
                                    stroke="rgb(var(--line-subtle) / var(--line-subtle-alpha))"
                                    strokeWidth={1}
                                    rx={3}
                                />
                                <text
                                    x={VALUE_SCALE_WIDTH / 2}
                                    y={localMouseY}
                                    textAnchor="middle"
                                    alignmentBaseline="middle"
                                    fill="rgb(var(--app-text))"
                                    fontSize={10}
                                    fontWeight="bold"
                                >
                                    {fmtVal(yVal, valDecimals)}
                                </text>
                            </>
                        );
                    })()}
            </svg>
        </div>
    );
};

export default LineChart;
