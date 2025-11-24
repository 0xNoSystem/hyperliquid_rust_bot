import { useRef, useEffect, useState } from "react";
import type { TimeFrame } from "../types";
import Candle from "./visual/Candle";
import CrossHair from "./visual/CrossHair";
import { useChartContext } from "./ChartContext";
import { fetchCandles, priceToY } from "./utils";
import type { CandleData } from "../chart/utils";

export interface ChartProps {
    asset: String;
    tf: TimeFrame;
    settingInterval: bool;
    candleData: CandleData[];
}

const Chart: React.FC<ChartProps> = ({ asset, tf, settingInterval, candleData}) => {
    const {
        setSize,
        setTf,
        setSelectingInterval,
        setMouseOnChart,
        setCrosshair,
        setPriceRange,
        setCandles,
        height,
        width,
        minPrice,
        maxPrice,
        candles,
    } = useChartContext();
    const candleWidth = Math.max(1, width / Math.max(1, candles.length));

    const [isInside, setIsInside] = useState(false);

    const ref = useRef<HTMLDivElement>(null);
    const [localSize, setLocalSize] = useState({ width: 0, height: 0 });

    const handleMouseMove = (e: React.MouseEvent<SVGSVGElement>) => {
        const rect = e.currentTarget.getBoundingClientRect();

        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;

        setCrosshair(x, y);
    };

    const handleMouseEnter = () => setIsInside(true);
    const handleMouseLeave = () => setIsInside(false);

    useEffect(() => {
        setMouseOnChart(isInside);
        setSelectingInterval(settingInterval);
        setTf(tf);
        setCandles(candleData);
    }, [candleData,isInside, settingInterval, tf]);

    useEffect(() => {
        if (candles.length === 0) return;

        const lows = candles.map((c) => c.low);
        const highs = candles.map((c) => c.high);

        setPriceRange(Math.min(...lows) -5, Math.max(...highs) + 5);
    }, [candles]);

    useEffect(() => {
        if (!ref.current) return;

        const observer = new ResizeObserver(([entry]) => {
            const { width, height } = entry.contentRect;

            setLocalSize({ width, height });

            setSize(width, height);
        });

        observer.observe(ref.current);
        return () => observer.disconnect();
    }, [setSize]);

    return (
        <div ref={ref} className="relative h-full flex-1 cursor-crosshair">
            <svg
                width={localSize.width}
                height={localSize.height}
                onMouseMove={handleMouseMove}
                onMouseEnter={handleMouseEnter}
                onMouseLeave={handleMouseLeave}
                className="min-h-full min-w-full"
            >
                <g>
                    {candles.map((c, i) => {
                        const x = i * candleWidth;

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

                        const bodyTop = Math.min(yOpen, yClose);
                        const bodyHeight = Math.abs(yOpen - yClose);

                        const wickTop = yHigh;
                        const wickHeight = yLow - yHigh;

                        const isGreen = c.close >= c.open;

                        return (
                            <Candle
                                key={c.start}
                                x={x}
                                width={candleWidth}
                                bodyTop={bodyTop}
                                bodyHeight={bodyHeight}
                                wickTop={wickTop}
                                wickHeight={wickHeight}
                                color={isGreen ? "white" : "gray"}
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
