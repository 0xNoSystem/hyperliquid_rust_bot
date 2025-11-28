import { createContext, useContext, useState } from "react";
import type { TimeFrame } from "../types";
import type { CandleData } from "./utils";

export const ChartContext = createContext<
    ChartContextState & ChartContextActions
>({} as ChartContextState & ChartContextActions);

interface ChartContextState {
    width: number;
    height: number;

    candles: CandleData[];
    timeframe: TimeFrame;

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

    setPriceRange: (min: number, max: number) => void;
    setManualPriceRange: (manual: boolean) => void;

    setTimeRange: (start: number, end: number) => void;

    setCrosshair: (x: number | null, y: number | null) => void;

    setSelectingInterval: (bool: boolean) => void;
    setIntervalStartX: (x: number | null) => void;
    setIntervalEndX: (x: number | null) => void;

    setMouseOnChart: (y: boolean) => void;
}

export default function ChartProvider({ children }) {
    const [width, setWidth] = useState(0);
    const [height, setHeight] = useState(0);

    const [timeframe, setTimeframe] = useState<TimeFrame | null>(null);
    const [candles, setCandles] = useState<CandleData[]>([]);

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
        setStartTime(start);
        setEndTime(end);
    };

    const setCrosshair = (x: number | null, y: number | null) => {
        setCrosshairX(x);
        setCrosshairY(y);
    };

    return (
        <ChartContext.Provider
            value={{
                // state
                width,
                height,

                candles,
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
