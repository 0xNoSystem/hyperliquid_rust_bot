import { useCallback, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { TimeFrame } from "../types";
import type { CandleData } from "./utils";
import { ChartContext } from "./ChartContextStore";

type ChartProviderProps = { children: ReactNode };

export default function ChartProvider({ children }: ChartProviderProps) {
    const [width, setWidth] = useState(0);
    const [height, setHeight] = useState(0);

    const [timeframe, setTimeframe] = useState<TimeFrame | null>(null);
    const [candles, setCandles] = useState<CandleData[]>([]);

    const [candleColor, setCandleColorState] = useState<{
        up: string;
        down: string;
    }>({
        up: "#cf7b15",
        down: "#c4c3c2",
    });

    const [minPrice, setMinPrice] = useState(0);
    const [maxPrice, setMaxPrice] = useState(0);
    const [manualPriceRange, setManualPriceRangeState] = useState(false);

    const [startTime, setStartTime] = useState(0);
    const [endTime, setEndTime] = useState(0);

    const [crosshairX, setCrosshairX] = useState<number | null>(null);
    const [crosshairY, setCrosshairY] = useState<number | null>(null);

    const [selectingInterval, setSelectingInterval] = useState(false);
    const [intervalStartX, setIntervalStartX] = useState<number | null>(null);
    const [intervalEndX, setIntervalEndX] = useState<number | null>(null);

    const [mouseOnChart, setMouseOnChart] = useState(false);
    const candleBounds = useMemo(() => {
        if (!candles.length) return null;
        let min = candles[0].start;
        let max = candles[0].end;
        for (const candle of candles) {
            if (candle.start < min) min = candle.start;
            if (candle.end > max) max = candle.end;
        }
        if (!Number.isFinite(min) || !Number.isFinite(max) || max <= min) {
            return null;
        }
        let candleDuration = 0;
        for (let i = 0; i < Math.min(10, candles.length); i++) {
            const d = candles[i].end - candles[i].start;
            if (Number.isFinite(d) && d > 0) {
                candleDuration = d;
                break;
            }
        }
        if (!candleDuration && candles.length > 0) {
            candleDuration = (max - min) / candles.length;
        }
        const range = max - min;
        const padding = Math.max(candleDuration * 150, range * 0.75);
        return {
            min,
            max,
            padding,
            paddedMin: min - padding,
            paddedMax: max + padding,
            maxRange: range + padding * 2,
        };
    }, [candles]);
    const candleBoundsRef = useRef<typeof candleBounds>(null);
    candleBoundsRef.current = candleBounds;

    // --- ACTIONS ---

    const setSize = useCallback((w: number, h: number) => {
        setWidth(w);
        setHeight(h);
    }, []);

    const setTf = useCallback((tf: TimeFrame) => setTimeframe(tf), []);

    const setPriceRange = useCallback((min: number, max: number) => {
        setMinPrice(min);
        setMaxPrice(max);
    }, []);
    const setManualPriceRange = useCallback(
        (manual: boolean) => setManualPriceRangeState(manual),
        []
    );

    const setTimeRange = useCallback((start: number, end: number) => {
        if (!Number.isFinite(start) || !Number.isFinite(end)) return;
        let s = start;
        let e = end;
        if (s > e) {
            const tmp = s;
            s = e;
            e = tmp;
        }
        const bounds = candleBoundsRef.current;
        if (bounds) {
            const { paddedMin, paddedMax, maxRange } = bounds;
            const desiredRange = e - s;
            if (desiredRange >= maxRange) {
                s = paddedMin;
                e = paddedMax;
            } else {
                if (s < paddedMin) {
                    const shift = paddedMin - s;
                    s += shift;
                    e += shift;
                }
                if (e > paddedMax) {
                    const shift = e - paddedMax;
                    s -= shift;
                    e -= shift;
                }
                s = Math.max(paddedMin, s);
                e = Math.min(paddedMax, e);
            }
        }
        setStartTime(s);
        setEndTime(e);
    }, []);

    const setCrosshair = useCallback((x: number | null, y: number | null) => {
        setCrosshairX(x);
        setCrosshairY(y);
    }, []);

    const setCandleColor = useCallback(
        (up: string | null, down: string | null) => {
            setCandleColorState((prev) => ({
                up: up ?? prev.up,
                down: down ?? prev.down,
            }));
        },
        []
    );

    return (
        <ChartContext.Provider
            value={{
                // state
                width,
                height,

                candles,
                candleColor,
                timeframe,

                minPrice,
                maxPrice,
                manualPriceRange,

                startTime,
                endTime,

                crosshairX,
                crosshairY,

                selectingInterval,
                intervalStartX,
                intervalEndX,

                mouseOnChart,

                // actions
                setSize,
                setTf,
                setCandles,
                setCandleColor,
                setPriceRange,
                setManualPriceRange,
                setTimeRange,
                setCrosshair,
                setSelectingInterval,
                setIntervalStartX,
                setIntervalEndX,
                setMouseOnChart,
            }}
        >
            {children}
        </ChartContext.Provider>
    );
}
