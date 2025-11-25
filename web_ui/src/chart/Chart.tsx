import React, { useRef, useEffect, useState, useMemo } from "react";
import Candle from "./visual/Candle";
import CrossHair from "./visual/CrossHair";
import { useChartContext } from "./ChartContext";

import { priceToY, timeToX, computeTimeWheelZoom, computeTimePan } from "./utils";
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

        height,
        width,
        minPrice,
        maxPrice,
        manualPriceRange,
        startTime,
        endTime,
    } = useChartContext();

    const [isInside, setIsInside] = useState(false);

    const containerRef = useRef<HTMLDivElement>(null);
    const [localSize, setLocalSize] = useState({ width: 0, height: 0 });

    // ------------------------------------------------------------
    // Set TF + interval mode
    // ------------------------------------------------------------
    useEffect(() => {
        setTf(tf);
        setSelectingInterval(settingInterval);
    }, [tf, settingInterval]);

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
        setTimeRange(candles[0].start, candles[candles.length - 1].end);
    }, [candles, setTimeRange, setManualPriceRange]);

    // ------------------------------------------------------------
    // Auto price range
    // ------------------------------------------------------------
    useEffect(() => {
        if (visibleCandles.length === 0 || manualPriceRange) return;

        const lows = visibleCandles.map((c) => c.low);
        const highs = visibleCandles.map((c) => c.high);

        setPriceRange(Math.min(...lows) * 0.98, Math.max(...highs) * 1.02);
    }, [visibleCandles, manualPriceRange, setPriceRange]);

    useEffect(() => {
    setManualPriceRange(false);
    }, [tf, setManualPriceRange]);

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
    // Wheel zoom / horizontal pan
    // ------------------------------------------------------------
    const onWheel = (e: React.WheelEvent) => {
        e.preventDefault();
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

        const move = (ev: MouseEvent) => {
            const dx = ev.clientX - startX;

            const { start, end } = computeTimePan(
                initialStart,
                initialEnd,
                dx,
                width
            );

            if (rawCandleWidth >= MAX_CANDLE_WIDTH && dx < 0) {
                return;
            }

            setTimeRange(start, end);
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
        setCrosshair(x, y);
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
    return (
        <div
            ref={containerRef}
            className="relative flex-1 cursor-crosshair"
            onWheel={onWheel}
            onMouseDown={onMouseDown}
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
                                color={c.close >= c.open ? "orange" : "white"}
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
