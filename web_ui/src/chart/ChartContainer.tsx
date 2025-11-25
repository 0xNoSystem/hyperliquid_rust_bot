import React, { useRef, useEffect, useState } from "react";
import Chart from "./Chart";
import type { ChartProps } from "./Chart";
import PriceScale from "./visual/PriceScale";
import TimeScale from "./visual/TimeScale";
import { useChartContext } from "./ChartContext";

interface ChartContainerProps extends ChartProps {}

const ChartContainer: React.FC<ChartContainerProps> = ({
    asset,
    timeframe,
    settingInterval,
    candleData,
}) => {
    const { crosshairX, crosshairY, startTime, height } = useChartContext();
    const rightRef = useRef<HTMLDivElement>(null);
    const [rightWidth, setRightWidth] = useState(0);

    useEffect(() => {
        if (!rightRef.current) return;

        const observer = new ResizeObserver(([entry]) => {
            const { width } = entry.contentRect;
            setRightWidth(width);
        });

        observer.observe(rightRef.current);
        return () => observer.disconnect();
    }, []);

    return (
        <div className="flex h-full flex-1 flex-col overflow-hidden">
            {/* MAIN ROW */}
            <div className="flex h-full w-[100%] flex-1">
                {/* LEFT: CHART */}
                <div className="relative flex w-[93%] flex-1 overflow-hidden">
                    <div
                        className={`absolute top-0 left-[20%] bg-gray-400/30`}
                        style={{
                            width: 400,
                            height: height,
                        }}
                    ></div>
                    <div className="relative flex flex-1">
                        <Chart
                            asset={asset}
                            tf={timeframe}
                            settingInterval={settingInterval}
                            candleData={candleData}
                        />
                    </div>
                </div>

                {/* RIGHT PANEL */}
                <div
                    ref={rightRef}
                    className="w-[7%] cursor-n-resize bg-black/20 text-white"
                >
                    <PriceScale />
                </div>
            </div>

            {/* BOTTOM PANEL */}
            <div className="flex bg-black/20 text-white">
                {/* Bottom content */}
                <div className="flex-1 cursor-w-resize py-4">
                    <TimeScale />
                </div>

                {/* Dynamic width preview */}
                <div className="bg-black/60" style={{ width: rightWidth }} />
            </div>
        </div>
    );
};

export default ChartContainer;
