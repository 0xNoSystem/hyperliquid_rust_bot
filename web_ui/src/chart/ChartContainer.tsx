import React, { useRef, useEffect, useState } from "react";
import Chart from "./Chart";
import type { ChartProps } from "./Chart";
import { useChartContext } from "./ChartContext";

interface ChartContainerProps extends ChartProps {}

const ChartContainer: React.FC<ChartContainerProps> = ({
    asset,
    timeframe,
    settingInterval,
    candleData,
}) => {
    const { crosshairX, crosshairY } = useChartContext();
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
        <div className="flex h-full w-full flex-1 flex-col">
            {/* MAIN ROW */}
            <div className="flex h-full w-[100%] flex-1 flex-row">
                {/* LEFT: CHART */}
                <div className="relative w-[90%] flex-1">
                    <Chart
                        asset={asset}
                        tf={timeframe}
                        settingInterval={settingInterval}
                        candleData={candleData}
                    />
                </div>

                {/* RIGHT PANEL */}
                <div
                    ref={rightRef}
                    className="w-[10%] cursor-n-resize bg-black/20 text-white"
                >
                    <div className="p-4">{crosshairY}</div>
                </div>
            </div>

            {/* BOTTOM PANEL */}
            <div className="flex w-full bg-black/20 text-white">
                {/* Bottom content */}
                <div className="flex-1 cursor-w-resize p-4">{crosshairX}</div>

                {/* Dynamic width preview */}
                <div className="bg-black/60" style={{ width: rightWidth }} />
            </div>
        </div>
    );
};

export default ChartContainer;
