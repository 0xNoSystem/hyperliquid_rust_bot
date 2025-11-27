import React, { useRef, useEffect, useState, useMemo } from "react";
import Candle from "./visual/Candle";
import CrossHair from "./visual/CrossHair";
import { useChartContext } from "./ChartContext";

import {
    priceToY,
    timeToX,
    xToTime,
    computeTimeWheelZoom,
    computeTimePan,
    computePricePan,
} from "./utils";
import { MIN_CANDLE_WIDTH, MAX_CANDLE_WIDTH } from "./constants";
import type { TimeFrame } from "../types";

export interface ChartProps {
    asset: string;
    tf: TimeFrame;
    settingInterval: boolean;
}

const Chart: React.FC<ChartProps> = ({ asset, tf, settingInterval }) => {
    const {
        candles,
        setTf,
        setSize,
        setSelectingInterval,
        setMouseOnChart,
        setCrosshair,
        setPriceRange,
        setManualPriceRange,
        setTimeRange,
        setIntervalStartX,
        setIntervalEndX,

        height,
        width,
        minPrice,
        maxPrice,
        manualPriceRange,
        startTime,
        endTime,
        selectingInterval,
    } = useChartContext();

    const [isInside, setIsInside] = useState(false);

    const containerRef = useRef<HTMLDivElement>(null);
    const [localSize, setLocalSize] = useState({ width: 0, height: 0 });
    const touchState = useRef<{
        mode: "pan" | "pinch";
        touchId?: number;
        startX: number;
        startY: number;
        initialStart: number;
        initialEnd: number;
        initialMin: number;
        initialMax: number;
        startDistance?: number;
        anchorRatio?: number;
    } | null>(null);

    // ------------------------------------------------------------
    // Set TF + interval mode
    // ------------------------------------------------------------
    useEffect(() => {
        setTf(tf);
        setSelectingInterval(settingInterval);
        if (!settingInterval) {
            setIntervalStartX(null);
            setIntervalEndX(null);
        }
    }, [
        tf,
        settingInterval,
        setSelectingInterval,
        setIntervalStartX,
        setIntervalEndX,
    ]);

    // ------------------------------------------------------------
    // Visible candles
    // ------------------------------------------------------------
    const visibleCandles = useMemo(() => {
        return candles.filter((c) => c.end >= startTime && c.start <= endTime);
    }, [candles, startTime, endTime]);

    // ------------------------------------------------------------
    // Initialize time range when new candles arrive
    // ------------------------------------------------------------
    const dataSignatureRef = useRef<string>("");
    useEffect(() => {
        if (!candles.length) return;

        const signature = `${candles[0].start}-${candles[candles.length - 1].end}`;
        if (dataSignatureRef.current === signature) return;

        dataSignatureRef.current = signature;
        setManualPriceRange(false);
        setIntervalStartX(null);
        setIntervalEndX(null);
    }, [
        candles,
        setTimeRange,
        setManualPriceRange,
        setIntervalStartX,
        setIntervalEndX,
    ]);

    // ------------------------------------------------------------
    // Auto price range
    // ------------------------------------------------------------
    useEffect(() => {
        if (visibleCandles.length === 0 || manualPriceRange) return;

        const lows = visibleCandles.map((c) => c.low);
        const highs = visibleCandles.map((c) => c.high);

        setPriceRange(Math.min(...lows) * 0.98, Math.max(...highs) * 1.02);
    }, [visibleCandles, manualPriceRange, setPriceRange]);

    // ------------------------------------------------------------
    // Resize observer
    // ------------------------------------------------------------
    useEffect(() => {
        if (!containerRef.current) return;

        const obs = new ResizeObserver(([entry]) => {
            const { width, height } = entry.contentRect;
            setLocalSize({ width, height });
            setSize(width, height);
        });

        obs.observe(containerRef.current);
        return () => obs.disconnect();
    }, [setSize]);

    // ------------------------------------------------------------
    // Prevent scroll chaining
    // ------------------------------------------------------------
    useEffect(() => {
        const node = containerRef.current;
        if (!node) return;

        const block = (e: WheelEvent) => e.preventDefault();
        node.addEventListener("wheel", block, { passive: false });

        return () => node.removeEventListener("wheel", block);
    }, []);

    // ------------------------------------------------------------
    // Block native touch scrolling within chart area
    // ------------------------------------------------------------
    useEffect(() => {
        const node = containerRef.current;
        if (!node) return;

        const blockTouch = (e: TouchEvent) => e.preventDefault();
        node.addEventListener("touchstart", blockTouch, { passive: false });
        node.addEventListener("touchmove", blockTouch, { passive: false });

        return () => {
            node.removeEventListener("touchstart", blockTouch);
            node.removeEventListener("touchmove", blockTouch);
        };
    }, []);

    // ------------------------------------------------------------
    // Wheel zoom / horizontal pan
    // ------------------------------------------------------------
    const onWheel = (e: React.WheelEvent) => {
        e.stopPropagation();

        const wantsPan = e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY);

        if (wantsPan && width > 0) {
            // shift+wheel usually reports deltaY, trackpads report deltaX
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

        if (rawCandleWidth >= MAX_CANDLE_WIDTH && e.deltaY < 0) return;

        setTimeRange(start, end);
    };

    // ------------------------------------------------------------
    // Drag pan
    // ------------------------------------------------------------
    const onMouseDown = (e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();

        const initialStart = startTime;
        const initialEnd = endTime;
        const startX = e.clientX;
        const initialMin = minPrice;
        const initialMax = maxPrice;
        const startY = e.clientY;

        const move = (ev: MouseEvent) => {
            const dx = ev.clientX - startX;
            const dy = ev.clientY - startY;

            let nextStart = initialStart;
            let nextEnd = initialEnd;

            if (!(rawCandleWidth >= MAX_CANDLE_WIDTH && dx < 0) && width > 0) {
                const { start, end } = computeTimePan(
                    initialStart,
                    initialEnd,
                    dx,
                    width
                );
                nextStart = start;
                nextEnd = end;
            }

            let nextMin = initialMin;
            let nextMax = initialMax;
            if (height > 0 && initialMax !== initialMin) {
                const { min, max } = computePricePan(
                    initialMin,
                    initialMax,
                    dy,
                    height
                );
                nextMin = min;
                nextMax = max;
            }

            setTimeRange(nextStart, nextEnd);

            if (nextMin !== initialMin || nextMax !== initialMax) {
                setManualPriceRange(true);
                setPriceRange(nextMin, nextMax);
            }
        };

        const up = () => {
            window.removeEventListener("mousemove", move);
            window.removeEventListener("mouseup", up);
        };

        window.addEventListener("mousemove", move);
        window.addEventListener("mouseup", up);
    };
    // ------------------------------------------------------------
    // Crosshair
    // ------------------------------------------------------------
    const handleMove = (e: React.MouseEvent<SVGSVGElement>) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;
        if (!width || selectingInterval || visibleCandles.length === 0) {
            setCrosshair(x, y);
            return;
        }

        if (x < 0 || x > rect.width) {
            setCrosshair(x, y);
            return;
        }

        const hoverTime = xToTime(x, startTime, endTime, width);
        const candleDuration =
            visibleCandles[0].end - visibleCandles[0].start || 1;

        const steps = Math.round((hoverTime - startTime) / candleDuration);
        const snappedTime = startTime + steps * candleDuration;
        const clampedTime = Math.min(endTime, Math.max(startTime, snappedTime));
        const snapX = timeToX(clampedTime, startTime, endTime, width);
        setCrosshair(snapX, y);
    };

    const handleEnter = () => {
        setIsInside(true);
        setMouseOnChart(true);
    };

    const handleLeave = () => {
        setIsInside(false);
        setMouseOnChart(false);
    };

    // ------------------------------------------------------------
    // Candle width
    // ------------------------------------------------------------
    const rawCandleWidth = useMemo(() => {
        if (visibleCandles.length < 2) return MIN_CANDLE_WIDTH;

        const range = endTime - startTime;
        const pxPerMs = width / range;
        const cDur = visibleCandles[0].end - visibleCandles[0].start;

        return pxPerMs * cDur;
    }, [visibleCandles, width, startTime, endTime]);

    const candleWidth = Math.min(
        MAX_CANDLE_WIDTH,
        Math.max(MIN_CANDLE_WIDTH, rawCandleWidth)
    );

    // ------------------------------------------------------------
    // Touch zoom constraints
    // ------------------------------------------------------------
    const candleDurationMs = useMemo(() => {
        if (visibleCandles.length > 0) {
            const duration = visibleCandles[0].end - visibleCandles[0].start;
            return duration || 1;
        }
        return 0;
    }, [visibleCandles]);

    const minTimeRangeForMaxWidth = useMemo(() => {
        if (width <= 0 || candleDurationMs <= 0) return 0;
        return (candleDurationMs * width) / MAX_CANDLE_WIDTH;
    }, [width, candleDurationMs]);

    const minTimeRange = useMemo(() => {
        const base = candleDurationMs || 1;
        const clampRange = minTimeRangeForMaxWidth || 0;
        return Math.max(1, base, clampRange);
    }, [candleDurationMs, minTimeRangeForMaxWidth]);

    // ------------------------------------------------------------
    // Touch interactions (pan + pinch zoom)
    // ------------------------------------------------------------
    const startTouchPan = (touch: Touch) => {
        touchState.current = {
            mode: "pan",
            touchId: touch.identifier,
            startX: touch.clientX,
            startY: touch.clientY,
            initialStart: startTime,
            initialEnd: endTime,
            initialMin: minPrice,
            initialMax: maxPrice,
        };
    };

    const startTouchPinch = (t1: Touch, t2: Touch) => {
        const distance = Math.hypot(
            t2.clientX - t1.clientX,
            t2.clientY - t1.clientY
        );

        const rect = containerRef.current?.getBoundingClientRect();
        const centerX =
            rect && width > 0
                ? Math.min(
                      Math.max((t1.clientX + t2.clientX) / 2 - rect.left, 0),
                      rect.width
                  )
                : width / 2;

        const anchorRatio =
            width > 0 ? Math.min(Math.max(centerX / width, 0), 1) : 0.5;

        touchState.current = {
            mode: "pinch",
            startDistance: Math.max(1, distance),
            anchorRatio,
            startX: (t1.clientX + t2.clientX) / 2,
            startY: (t1.clientY + t2.clientY) / 2,
            initialStart: startTime,
            initialEnd: endTime,
            initialMin: minPrice,
            initialMax: maxPrice,
        };
    };

    const onTouchStart = (e: React.TouchEvent) => {
        e.stopPropagation();
        if (e.touches.length === 1) {
            startTouchPan(e.touches[0]);
        } else if (e.touches.length >= 2) {
            startTouchPinch(e.touches[0], e.touches[1]);
        }
    };

    const onTouchMove = (e: React.TouchEvent) => {
        e.stopPropagation();
        if (!touchState.current) {
            if (e.touches.length === 1) startTouchPan(e.touches[0]);
            else if (e.touches.length >= 2)
                startTouchPinch(e.touches[0], e.touches[1]);
        }
        if (!touchState.current) return;

        if (touchState.current.mode === "pan" && e.touches.length === 1) {
            const state = touchState.current;
            const touch =
                Array.from(e.touches).find(
                    (t) => t.identifier === state.touchId
                ) || e.touches[0];

            const dx = touch.clientX - state.startX;
            const dy = touch.clientY - state.startY;

            let nextStart = state.initialStart;
            let nextEnd = state.initialEnd;

            if (!(rawCandleWidth >= MAX_CANDLE_WIDTH && dx < 0) && width > 0) {
                const { start, end } = computeTimePan(
                    state.initialStart,
                    state.initialEnd,
                    dx,
                    width
                );
                nextStart = start;
                nextEnd = end;
            }

            let nextMin = state.initialMin;
            let nextMax = state.initialMax;
            if (height > 0 && nextMax !== nextMin) {
                const { min, max } = computePricePan(
                    state.initialMin,
                    state.initialMax,
                    dy,
                    height
                );
                nextMin = min;
                nextMax = max;
            }

            setTimeRange(nextStart, nextEnd);

            if (nextMin !== state.initialMin || nextMax !== state.initialMax) {
                setManualPriceRange(true);
                setPriceRange(nextMin, nextMax);
            }
            return;
        }

        if (e.touches.length >= 2) {
            const [t1, t2] = [e.touches[0], e.touches[1]];
            if (touchState.current.mode !== "pinch") {
                startTouchPinch(t1, t2);
                return;
            }

            const state = touchState.current;
            const distance = Math.hypot(
                t2.clientX - t1.clientX,
                t2.clientY - t1.clientY
            );
            const initialRange = state.initialEnd - state.initialStart;
            if (!state.startDistance || initialRange <= 0) return;

            let newRange =
                initialRange * (state.startDistance / Math.max(1, distance));
            newRange = Math.max(minTimeRange, newRange);

            const anchorRatio = state.anchorRatio ?? 0.5;
            const anchorTime =
                state.initialStart + anchorRatio * initialRange;
            const newStart = anchorTime - anchorRatio * newRange;
            const newEnd = newStart + newRange;

            setTimeRange(newStart, newEnd);
        }
    };

    const onTouchEnd = (e: React.TouchEvent) => {
        e.stopPropagation();
        if (e.touches.length === 1) {
            startTouchPan(e.touches[0]);
            return;
        }

        if (e.touches.length === 0) {
            touchState.current = null;
        }
    };

    // ------------------------------------------------------------
    return (
        <div
            ref={containerRef}
            className="relative flex-1 cursor-crosshair"
            style={{ touchAction: "none", overscrollBehavior: "contain" }}
            onWheel={onWheel}
            onMouseDown={onMouseDown}
            onTouchStart={onTouchStart}
            onTouchMove={onTouchMove}
            onTouchEnd={onTouchEnd}
            onTouchCancel={onTouchEnd}
        >
            <svg
                width={localSize.width}
                height={localSize.height}
                className="min-h-full min-w-full"
                onMouseMove={handleMove}
                onMouseEnter={handleEnter}
                onMouseLeave={handleLeave}
            >
                <g>
                    {visibleCandles.map((c) => {
                        const x = timeToX(c.start, startTime, endTime, width);

                        const yOpen = priceToY(
                            c.open,
                            minPrice,
                            maxPrice,
                            height
                        );
                        const yClose = priceToY(
                            c.close,
                            minPrice,
                            maxPrice,
                            height
                        );
                        const yHigh = priceToY(
                            c.high,
                            minPrice,
                            maxPrice,
                            height
                        );
                        const yLow = priceToY(
                            c.low,
                            minPrice,
                            maxPrice,
                            height
                        );

                        return (
                            <Candle
                                key={c.start}
                                x={x}
                                width={candleWidth}
                                bodyTop={Math.min(yOpen, yClose)}
                                bodyHeight={Math.abs(yOpen - yClose)}
                                wickTop={yHigh}
                                wickHeight={yLow - yHigh}
                                color={c.close >= c.open ? "#cf7b15" : "#c4c3c2"}
                            />
                        );
                    })}
                </g>

                {isInside && !settingInterval && <CrossHair />}
            </svg>
        </div>
    );
};

export default Chart;
