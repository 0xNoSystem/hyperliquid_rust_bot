import { createContext, useContext } from "react";
import type { TimeFrame } from "../types";
import type { CandleData } from "./utils";

export interface ChartContextState {
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

export interface ChartContextActions {
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

export type ChartContextValue = ChartContextState & ChartContextActions;

export const ChartContext = createContext<ChartContextValue>(
    {} as ChartContextValue
);

export function useChartContext() {
    return useContext(ChartContext);
}
