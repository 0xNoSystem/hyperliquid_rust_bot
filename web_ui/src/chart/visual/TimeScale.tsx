import React, { useRef, useEffect } from "react";
import { useChartContext } from "../ChartContext";
import { timeToX, xToTime, formatUTC, computeTimePan } from "../utils";
import { MAX_CANDLE_WIDTH } from "../constants";
import { TF_TO_MS } from "../../types";

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

    const ref = useRef<SVGSVGElement>(null);

    const ticks = 12;
    const step = (endTime - startTime) / (ticks - 1);

    const times = Array.from({ length: ticks }, (_, i) => {
        const t = startTime + i * step;
        const x = timeToX(t, startTime, endTime, width);
        return { t, x };
    });

    const crosshairTime =
        crosshairX !== null
            ? xToTime(crosshairX, startTime, endTime, width)
            : null;

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

    return (
        <div className="pb-2" onWheel={onWheel} onMouseDown={onMouseDown}>
            <svg
                ref={ref}
                width={width}
                height={25}
                style={{ overflow: "visible", overscrollBehavior: "none" }}
            >
                {/* Tick Labels */}
                {times.slice(0, -1).map((p, idx) => (
                    <g key={idx}>
                        <line
                            x1={p.x}
                            y1={0}
                            x2={p.x}
                            y2={-height - 10}
                            stroke="#444"
                            strokeOpacity={0.4}
                            strokeWidth={0.8}
                        />
                        <text
                            x={p.x}
                            y={20}
                            textAnchor="middle"
                            fill="#aaa"
                            fontSize={11}
                        >
                            {formatUTC(p.t)}
                        </text>
                    </g>
                ))}

                {/* Crosshair Time Label */}
                {crosshairTime !== null &&
                    mouseOnChart &&
                    !selectingInterval && (
                        <>
                            <rect
                                x={crosshairX - 60}
                                y={0}
                                width={120}
                                height={18}
                                fill="#2a2a2a"
                                stroke="#ffffff44"
                                strokeWidth={1}
                                rx={4}
                            />
                            <text
                                x={crosshairX}
                                y={13}
                                textAnchor="middle"
                                fill="white"
                                fontSize={12}
                                fontWeight="bold"
                            >
                                {formatUTC(crosshairTime)}
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
