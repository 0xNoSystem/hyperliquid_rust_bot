import React, { useRef, useEffect, useState } from "react";
import Chart from "./Chart";
import PriceScale from "./visual/PriceScale";
import TimeScale from "./visual/TimeScale";
import IntervalOverlay from "./visual/Interval";
import CandleInfo from "./visual/CandleInfo";
import { useChartContext } from "./ChartContext";
import { xToTime } from "./utils";

import type { TimeFrame, CandleData } from "../types";

interface ChartContainerProps {
    asset: string;
    tf: TimeFrame;
    settingInterval: boolean;
    candleData: CandleData[];
}

const ChartContainer: React.FC<ChartContainerProps> = ({
    asset,
    tf,
    settingInterval,
    candleData,
}) => {
    const {
        height,
        setCandles,
        selectingInterval,
        startTime,
        endTime,
        candles,
        width,
        crosshairX,
        mouseOnChart,
    } = useChartContext();
    const rightRef = useRef<HTMLDivElement>(null);
    const [rightWidth, setRightWidth] = useState(0);
    const [hoveredCandle, setHoveredCandle] = useState<CandleData | null>(null);

    // Load candle data into context
    useEffect(() => {
        if (!candleData) return;
        setCandles(candleData);
    }, [candleData, setCandles]);

    // Track right panel width (needed for bottom preview)
    useEffect(() => {
        if (!rightRef.current) return;

        const obs = new ResizeObserver(([entry]) => {
            setRightWidth(entry.contentRect.width);
        });

        obs.observe(rightRef.current);
        return () => obs.disconnect();
    }, []);

    useEffect(() => {
        if (
            selectingInterval ||
            crosshairX === null ||
            width === 0 ||
            endTime <= startTime ||
            candles.length === 0
        ) {
            setHoveredCandle(null);
            return;
        }

        const hoverTime = xToTime(crosshairX, startTime, endTime, width);

        let nearest: CandleData | null = null;
        let bestDiff = Infinity;

        for (const candle of candles) {
            if (candle.end < startTime || candle.start > endTime) continue;
            const mid = (candle.start + candle.end) / 2;
            const diff = Math.abs(mid - hoverTime);
            if (diff < bestDiff) {
                bestDiff = diff;
                nearest = candle;
            }
        }

        setHoveredCandle(nearest);
    }, [selectingInterval, crosshairX, width, startTime, endTime, candles]);

    return (
        <div className="flex h-full flex-1 flex-col overflow-hidden">
            {/* MAIN ROW */}
            <div className="flex h-full w-full flex-1">
                {/* LEFT: CHART */}
                <div className="relative flex w-[93%] flex-1 overflow-hidden">
                    <div className="relative flex flex-1">
                        <Chart
                            asset={asset}
                            tf={tf}
                            settingInterval={settingInterval}
                        />
                        <IntervalOverlay />
                        {hoveredCandle && mouseOnChart && (
                            <CandleInfo candle={hoveredCandle} />
                        )}
                    </div>
                </div>

                {/* RIGHT PRICE SCALE */}
                <div
                    ref={rightRef}
                    className="w-fit cursor-n-resize bg-black/20 text-white"
                >
                    <PriceScale />
                </div>
            </div>

            {/* BOTTOM TIME SCALE */}
            <div className="flex bg-black/20 text-white">
                <div className="flex-1 cursor-w-resize">
                    <TimeScale />
                </div>

                {/* Right-side width preview box */}
                <div className="bg-black/60" style={{ width: rightWidth }} />
            </div>
        </div>
    );
};

export default ChartContainer;
