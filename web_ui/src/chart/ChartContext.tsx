import { createContext, useContext, useMemo, useState } from "react";
import type { ReactNode } from "react";
import type { TimeFrame } from "../types";
import type { CandleData } from "./utils";

export const ChartContext = createContext<
    ChartContextState & ChartContextActions
>({} as ChartContextState & ChartContextActions);

interface ChartContextState {
    width: number;
    height: number;

    candles: CandleData[];
    candleColor: { up: string; down: string };
    timeframe: TimeFrame | null;

    minPrice: number;
    maxPrice: number;
    manualPriceRange: boolean;

    startTime: number;
    endTime: number;

    crosshairX: number | null;
    crosshairY: number | null;

    selectingInterval: boolean;
    intervalStartX: number | null;
    intervalEndX: number | null;

    mouseOnChart: boolean;
}

interface ChartContextActions {
    setSize: (w: number, h: number) => void;
    setTf: (tf: TimeFrame) => void;

    setCandles: (c: CandleData[]) => void;
    setCandleColor: (up: string | null, down: string | null) => void;

    setPriceRange: (min: number, max: number) => void;
    setManualPriceRange: (manual: boolean) => void;

    setTimeRange: (start: number, end: number) => void;

    setCrosshair: (x: number | null, y: number | null) => void;

    setSelectingInterval: (bool: boolean) => void;
    setIntervalStartX: (x: number | null) => void;
    setIntervalEndX: (x: number | null) => void;

    setMouseOnChart: (y: boolean) => void;
}

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

    // --- ACTIONS ---

    const setSize = (w: number, h: number) => {
        setWidth(w);
        setHeight(h);
    };

    const setTf = (tf: TimeFrame) => {
        setTimeframe(tf);
    };

    const setPriceRange = (min: number, max: number) => {
        setMinPrice(min);
        setMaxPrice(max);
    };
    const setManualPriceRange = (manual: boolean) => {
        setManualPriceRangeState(manual);
    };

    const setTimeRange = (start: number, end: number) => {
        if (!Number.isFinite(start) || !Number.isFinite(end)) return;
        let s = start;
        let e = end;
        if (s > e) {
            const tmp = s;
            s = e;
            e = tmp;
        }
        if (candleBounds) {
            const { paddedMin, paddedMax, maxRange } = candleBounds;
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
    };

    const setCrosshair = (x: number | null, y: number | null) => {
        setCrosshairX(x);
        setCrosshairY(y);
    };

    const setCandleColor = (up: string | null, down: string | null) => {
        setCandleColorState((prev) => ({
            up: up ?? prev.up,
            down: down ?? prev.down,
        }));
    };

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

export function useChartContext() {
    return useContext(ChartContext);
}
