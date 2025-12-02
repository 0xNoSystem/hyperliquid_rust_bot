import React, {
    useRef,
    useEffect,
    useState,
    useMemo,
    useCallback,
} from "react";
import CrossHair from "./visual/CrossHair";
import { useChartContext } from "./ChartContext";

import {
    priceToY,
    timeToX,
    computeTimeWheelZoom,
    computeTimePan,
    computePricePan,
} from "./utils";
import { MIN_CANDLE_WIDTH, MAX_CANDLE_WIDTH } from "./constants";
import type { TimeFrame } from "../types";
import type { CandleData } from "./utils";

export interface ChartProps {
    asset: string;
    tf: TimeFrame;
    settingInterval: boolean;
}

type CandleCanvasProps = {
    width: number;
    height: number;
    candles: CandleData[];
    candleColor: {up: string, down: string};
    startTime: number;
    endTime: number;
    minPrice: number;
    maxPrice: number;
    candleWidth: number;
    className?: string;
    onMouseMove?: (e: React.MouseEvent<HTMLCanvasElement>) => void;
    onMouseEnter?: (e: React.MouseEvent<HTMLCanvasElement>) => void;
    onMouseLeave?: (e: React.MouseEvent<HTMLCanvasElement>) => void;
};

const CandleCanvas: React.FC<CandleCanvasProps> = ({
    width,
    height,
    candles,
    candleColor,
    startTime,
    endTime,
    minPrice,
    maxPrice,
    candleWidth,
    className,
    onMouseMove,
    onMouseEnter,
    onMouseLeave,
}) => {
    const canvasRef = useRef<HTMLCanvasElement | null>(null);
    const rafRef = useRef<number | null>(null);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;

        const cssWidth = Math.max(0, width);
        const cssHeight = Math.max(0, height);
        if (cssWidth === 0 || cssHeight === 0) {
            canvas.width = 0;
            canvas.height = 0;
            return;
        }

        const dpr = Math.max(1, window.devicePixelRatio || 1);
        canvas.style.width = `${cssWidth}px`;
        canvas.style.height = `${cssHeight}px`;

        const targetWidth = Math.floor(cssWidth * dpr);
        const targetHeight = Math.floor(cssHeight * dpr);

        if (canvas.width !== targetWidth || canvas.height !== targetHeight) {
            canvas.width = targetWidth;
            canvas.height = targetHeight;
        }

        const ctx = canvas.getContext("2d");
        if (!ctx) return;

        const draw = () => {
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
            ctx.clearRect(0, 0, cssWidth, cssHeight);

            if (candles.length === 0) return;
            if (endTime <= startTime) return;

            const upColor = candleColor.up;
            const downColor = candleColor.down;
            const wickWidth = candleWidth / 2 <= 1 ? 0.2 : 1;

            const drawWicks = (isUp: boolean, color: string) => {
                ctx.beginPath();
                for (let i = 0; i < candles.length; i++) {
                    const cd = candles[i];
                    if (cd.close >= cd.open !== isUp) continue;

                    const centerX =
                        timeToX(cd.start, startTime, endTime, cssWidth) +
                        candleWidth / 2;
                    const lineX = Math.round(centerX) + 0.5;
                    const yHigh = priceToY(
                        cd.high,
                        minPrice,
                        maxPrice,
                        cssHeight
                    );
                    const yLow = priceToY(
                        cd.low,
                        minPrice,
                        maxPrice,
                        cssHeight
                    );

                    ctx.moveTo(lineX, yHigh);
                    ctx.lineTo(lineX, yLow);
                }
                ctx.lineWidth = wickWidth;
                ctx.strokeStyle = color;
                ctx.stroke();
            };

            const drawBodies = (isUp: boolean, color: string) => {
                ctx.fillStyle = color;
                for (let i = 0; i < candles.length; i++) {
                    const cd = candles[i];
                    if (cd.close >= cd.open !== isUp) continue;

                    const x = timeToX(cd.start, startTime, endTime, cssWidth);
                    const yOpen = priceToY(
                        cd.open,
                        minPrice,
                        maxPrice,
                        cssHeight
                    );
                    const yClose = priceToY(
                        cd.close,
                        minPrice,
                        maxPrice,
                        cssHeight
                    );
                    const top = Math.min(yOpen, yClose);
                    const heightPx = Math.max(1, Math.abs(yOpen - yClose));

                    ctx.fillRect(x, top, candleWidth, heightPx);
                }
            };

            drawWicks(true, upColor);
            drawWicks(false, downColor);
            drawBodies(true, upColor);
            drawBodies(false, downColor);
        };

        if (rafRef.current !== null) {
            cancelAnimationFrame(rafRef.current);
        }
        rafRef.current = requestAnimationFrame(draw);

        return () => {
            if (rafRef.current !== null) {
                cancelAnimationFrame(rafRef.current);
            }
        };
    }, [
        width,
        height,
        candles,
        candleColor,
        startTime,
        endTime,
        minPrice,
        maxPrice,
        candleWidth,
    ]);

    return (
        <canvas
            ref={canvasRef}
            className={className}
            onMouseMove={onMouseMove}
            onMouseEnter={onMouseEnter}
            onMouseLeave={onMouseLeave}
        />
    );
};

function lowerBound<T>(
    arr: readonly T[],
    target: number,
    key: (x: T) => number
) {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (key(arr[mid]) < target) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

function upperBound<T>(
    arr: readonly T[],
    target: number,
    key: (x: T) => number
) {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (key(arr[mid]) <= target) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

function aggregateCandles(c: CandleData[], groupSize: number): CandleData[] {
    if (groupSize <= 1) return c;
    const out: CandleData[] = [];
    for (let i = 0; i < c.length; i += groupSize) {
        const startCandle = c[i];
        const lastIdx = Math.min(i + groupSize - 1, c.length - 1);
        const endCandle = c[lastIdx];

        let high = -Infinity;
        let low = Infinity;
        let volume = 0;
        let trades = 0;
        for (let j = i; j <= lastIdx; j++) {
            const item = c[j];
            if (item.high > high) high = item.high;
            if (item.low < low) low = item.low;
            volume += item.volume;
            trades += item.trades;
        }

        out.push({
            start: startCandle.start,
            end: endCandle.end,
            open: startCandle.open,
            high,
            low,
            close: endCandle.close,
            volume,
            trades,
            asset: startCandle.asset,
            interval: startCandle.interval,
        });
    }
    return out;
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
        candleColor,
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
    const wheelBusy = useRef(false);

    const containerRef = useRef<HTMLDivElement>(null);
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
    const visibleRange = useMemo(() => {
        if (candles.length === 0 || endTime <= startTime) {
            return { first: 0, lastExcl: 0 };
        }
        const first = lowerBound(candles, startTime, (c) => c.end);
        const lastExcl = upperBound(candles, endTime, (c) => c.start);
        return { first, lastExcl };
    }, [candles, startTime, endTime]);

    const visibleCandles = useMemo(() => {
        if (visibleRange.lastExcl <= visibleRange.first) return [];
        return candles.slice(visibleRange.first, visibleRange.lastExcl);
    }, [candles, visibleRange]);

    // ------------------------------------------------------------
    // Candle width + LOD
    // ------------------------------------------------------------
    const rawCandleWidth = useMemo(() => {
        if (visibleCandles.length === 0 || width <= 0) return 0;

        const range = endTime - startTime;
        if (range <= 0) return 0;
        const pxPerMs = width / Math.max(range, 1);
        const cDur = visibleCandles[0].end - visibleCandles[0].start;

        return pxPerMs * cDur;
    }, [visibleCandles, width, startTime, endTime]);

    const barsPerPx =
        width > 0 ? visibleCandles.length / Math.max(1, width) : 0;
    const lodK = barsPerPx > 8 ? 16 : barsPerPx > 4 ? 8 : barsPerPx > 2 ? 4 : 1;

    const drawCandles = useMemo(() => {
        return lodK === 1
            ? visibleCandles
            : aggregateCandles(visibleCandles, lodK);
    }, [lodK, visibleCandles]);

    const minSpacingPx = useMemo(() => {
        if (drawCandles.length < 2 || width <= 0 || endTime <= startTime) {
            return null;
        }

        let spacing = Infinity;
        for (let i = 1; i < drawCandles.length; i++) {
            const prevX = timeToX(
                drawCandles[i - 1].start,
                startTime,
                endTime,
                width
            );
            const x = timeToX(drawCandles[i].start, startTime, endTime, width);
            const gap = x - prevX;
            if (gap > 0 && gap < spacing) spacing = gap;
        }

        if (!Number.isFinite(spacing) || spacing <= 0) return null;
        return spacing;
    }, [drawCandles, endTime, startTime, width]);

    const targetCandleWidth = rawCandleWidth * lodK;
    const widthRespectingSpacing =
        minSpacingPx === null
            ? targetCandleWidth
            : Math.min(targetCandleWidth, minSpacingPx * 0.9);
    const renderCandleWidth = Math.min(
        MAX_CANDLE_WIDTH,
        minSpacingPx === null
            ? Math.max(MIN_CANDLE_WIDTH, widthRespectingSpacing)
            : Math.max(0, widthRespectingSpacing)
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

    const applyTimeRange = useCallback(
        (start: number, end: number) => {
            if (!Number.isFinite(start) || !Number.isFinite(end)) return;
            let s = start;
            let e = end;
            if (s > e) {
                const tmp = s;
                s = e;
                e = tmp;
            }
            const minRange = Math.max(1, minTimeRange);
            const currentRange = e - s;
            if (currentRange < minRange) {
                const mid = (s + e) / 2;
                s = mid - minRange / 2;
                e = mid + minRange / 2;
            }
            setTimeRange(s, e);
        },
        [minTimeRange, setTimeRange]
    );

    // ------------------------------------------------------------
    // Initialize time range when new candles arrive
    // ------------------------------------------------------------
    const dataSignatureRef = useRef<string>("");
    useEffect(() => {
        if (!candles.length) return;

        const signature = `${asset}-${candles[0].start}-${candles[candles.length - 1].end}`;
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

        let low = Infinity;
        let high = -Infinity;
        for (let i = 0; i < visibleCandles.length; i++) {
            const c = visibleCandles[i];
            if (c.low < low) low = c.low;
            if (c.high > high) high = c.high;
        }

        if (Number.isFinite(low) && Number.isFinite(high) && low < high) {
            setPriceRange(low * 0.98, high * 1.02);
        }
    }, [visibleCandles, manualPriceRange, setPriceRange]);

    // ------------------------------------------------------------
    // Resize observer
    // ------------------------------------------------------------
    useEffect(() => {
        if (!containerRef.current) return;

        const obs = new ResizeObserver(([entry]) => {
            const { width, height } = entry.contentRect;
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
        e.preventDefault();
        e.stopPropagation();
        if (wheelBusy.current) return;
        wheelBusy.current = true;
        requestAnimationFrame(() => {
            wheelBusy.current = false;
        });

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

            applyTimeRange(start, end);
            return;
        }

        const { start, end } = computeTimeWheelZoom(
            startTime,
            endTime,
            e.deltaY
        );

        if (rawCandleWidth >= MAX_CANDLE_WIDTH && e.deltaY < 0) return;

        applyTimeRange(start, end);
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

            applyTimeRange(nextStart, nextEnd);

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
    const handleMove = (e: React.MouseEvent<Element>) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const effectiveWidth = width || rect.width || 0;
        const effectiveHeight = height || rect.height || 0;
        const scaleX = rect.width ? effectiveWidth / rect.width : 1;
        const scaleY = rect.height ? effectiveHeight / rect.height : 1;

        // Normalize mouse position back into the chart's logical pixel space.
        const rawX = (e.clientX - rect.left) * scaleX;
        const rawY = (e.clientY - rect.top) * scaleY;
        const x = Math.min(Math.max(rawX, 0), effectiveWidth);
        const y = Math.min(Math.max(rawY, 0), effectiveHeight);

        if (
            !effectiveWidth ||
            !effectiveHeight ||
            selectingInterval ||
            drawCandles.length === 0 ||
            endTime <= startTime
        ) {
            setCrosshair(x, y);
            return;
        }

        const pxPerMs = effectiveWidth / Math.max(endTime - startTime, 1);
        let bestX = x;
        let bestDiff = Infinity;

        for (let i = 0; i < drawCandles.length; i++) {
            const startOffset = (drawCandles[i].start - startTime) * pxPerMs;
            const centerX = startOffset + renderCandleWidth / 2;
            const diff = Math.abs(centerX - x);

            if (diff < bestDiff) {
                bestDiff = diff;
                bestX = centerX;
            } else if (centerX > x && diff > bestDiff) {
                break;
            }
        }

        const snappedX = Math.min(Math.max(bestX, 0), effectiveWidth);
        setCrosshair(snappedX, y);
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

            applyTimeRange(nextStart, nextEnd);

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
            const anchorTime = state.initialStart + anchorRatio * initialRange;
            const newStart = anchorTime - anchorRatio * newRange;
            const newEnd = newStart + newRange;

            applyTimeRange(newStart, newEnd);
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
            className="relative max-h-[70vh] h-[60vh] flex-1 cursor-crosshair"
            style={{ touchAction: "none", overscrollBehavior: "contain" }}
            onWheel={onWheel}
            onMouseDown={onMouseDown}
            onTouchStart={onTouchStart}
            onTouchMove={onTouchMove}
            onTouchEnd={onTouchEnd}
            onTouchCancel={onTouchEnd}
        >
            <CandleCanvas
                width={width}
                height={height}
                candles={drawCandles}
                candleColor={candleColor}
                startTime={startTime}
                endTime={endTime}
                minPrice={minPrice}
                maxPrice={maxPrice}
                candleWidth={renderCandleWidth}
                className="absolute inset-0 h-full w-full"
                onMouseMove={handleMove}
                onMouseEnter={handleEnter}
                onMouseLeave={handleLeave}
            />

            <svg
                width={width}
                height={height}
                className="pointer-events-none absolute inset-0"
            >
                {isInside && !settingInterval && <CrossHair />}
            </svg>
        </div>
    );
};

export default Chart;
