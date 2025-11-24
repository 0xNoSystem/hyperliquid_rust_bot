import React, { useState, useEffect } from "react";
import { useParams } from "react-router-dom";
import { TIMEFRAME_CAMELCASE, fromTimeFrame, TF_TO_MS } from "../types";
import type { TimeFrame } from "../types";
import ChartContainer from "../chart/ChartContainer";
import ChartProvider from "../chart/ChartContext";
import { fetchCandles } from "../chart/utils";
import type { CandleData} from "../chart/utils";

async function loadCandles(
    tf: TimeFrame,
    intervalOn: boolean,
    startDate: string,
    endDate: string
): Promise<CandleData[]> {
    let startMs: number;
    let endMs: number;

    // If user selected a custom time range
    if (startDate && endDate) {
        startMs = new Date(startDate).getTime();
        endMs = new Date(endDate).getTime();
    }
    // Otherwise load default recent data
    else {
        endMs = Date.now();
        startMs = endMs - 30 * 60 * 1000; // last 30 minutes
    }

    // Compute expected candle count
    const candleIntervalMs = TF_TO_MS[tf];
    const expectedCandles = Math.ceil((endMs - startMs) / candleIntervalMs);

    console.log(
        `%c[LOAD CANDLES] TF=${tf}, Expected=${expectedCandles}`,
        "color: orange; font-weight: bold;"
    );

    // Binance fetch (with automatic batching)
    return await fetchCandles("SOL", startMs, endMs, fromTimeFrame(tf));
}



// -----------------------
// Backtest Component
// -----------------------
export default function Backtest() {
    const { asset: routeAsset } = useParams<{ asset: string }>();

    const [timeframe, setTimeframe] = useState<TimeFrame>("min1");
    const [intervalOn, setIntervalOn] = useState(false);
    const [candleData, setCandleData] = useState<CandleData[]>([]);

    const [startDate, setStartDate] = useState<string>("");
    const [endDate, setEndDate] = useState<string>("");

    // Auto-reload candles when TF, toggle, or date inputs change
    useEffect(() => {
        async function reload() {
            const data = await loadCandles(
                timeframe,
                intervalOn,
                startDate,
                endDate
            );
            setCandleData(data);
        }

        reload();
    }, [timeframe, startDate, endDate]);

    return (
        <div className="flex h-full flex-col bg-black/70 pb-50">
            {/* Title */}
            <h1 className="mt-6 p-2 text-center text-3xl font-bold tracking-widest">
                STRATEGY LAB
            </h1>

            {/* Layout */}
            <div className="z-1 flex flex-grow flex-col items-center justify-between py-8">

                {/* STRATEGY (top) */}
                <div className="mb-6 mb-30 w-[60%] border-2 border-white/70 bg-black/60 p-4 text-center tracking-widest">
                    <h2 className="p-2 text-xl font-semibold">Strategy</h2>
                </div>

                {/* CHART (middle) */}
                <div className="mb-6 mb-30 flex min-h-[70vh] w-[90%] flex-grow flex-col rounded-lg border-2 border-white/20  p-4 tracking-widest">

                    {/* Toggle + Dates */}
                    <div className="flex p-4 pl-1 items-center gap-4">
                        {/* Toggle Button */}
                        <button
                            onClick={() => setIntervalOn(!intervalOn)}
                            className={`relative mr-3 flex h-6 w-12 cursor-pointer items-center rounded-full transition-colors duration-300 ${
                                intervalOn
                                    ? "bg-orange-500"
                                    : "bg-gray-600"
                            }`}
                        >
                            <span
                                className={`absolute top-1 left-1 h-4 w-4 rounded-full bg-white transition-transform duration-300 ${
                                    intervalOn
                                        ? "translate-x-6"
                                        : "translate-x-0"
                                }`}
                            />
                        </button>

                        <h3 className="tracking-wide">
                            Select BT period {intervalOn ? "On" : "Off"}
                        </h3>

                        {/* START DATE */}
                        <input
                            type="datetime-local"
                            value={startDate}
                            onChange={(e) => setStartDate(e.target.value)}
                            className={`p-1 rounded bg-black/40 text-white border border-white/40
                            }`}
                        />

                        {/* END DATE */}
                        <input
                            type="datetime-local"
                            value={endDate}
                            onChange={(e) => setEndDate(e.target.value)}
                            className={`p-1 rounded bg-black/40 text-white border border-white/40
                            }`}
                        />
                    </div>

                    {/* Asset Title */}
                    <h2 className="rounded-t-lg bg-black/80 p-2 text-center text-2xl font-semibold">
                        {routeAsset}
                    </h2>

                    {/* TF SELECTOR */}
                    <div className="flex flex-1 flex-col rounded-b-lg border-2 border-black/30 bg-[#111212]">

                        <div className="z-5 grid w-full grid-cols-13 bg-black/70 text-center tracking-normal">
                            {Object.entries(TIMEFRAME_CAMELCASE).map(
                                ([short, tf]) => (
                                    <div
                                        className="border-b-2 border-black py-2 text-white/70 hover:bg-black cursor-pointer"
                                        key={short}
                                        onClick={() => {
                                            setTimeframe(tf);
                                        }}
                                    >
                                        <span
                                            className={`px-2 text-center text-sm ${
                                                timeframe === tf
                                                    ? "font-bold text-orange-500"
                                                    : ""
                                            }`}
                                        >
                                            {short}
                                        </span>
                                    </div>
                                )
                            )}
                        </div>

                        {/* CHART PROVIDER + CHART */}
                        <ChartProvider>
                            <ChartContainer
                                asset={routeAsset}
                                timeframe={timeframe}
                                settingInterval={intervalOn}
                                candleData={candleData}
                            />
                        </ChartProvider>
                    </div>
                </div>

                {/* CONSOLE (bottom) */}
                <div className="w-[60%] border-2 border-white/70 bg-black/60 p-2 text-center text-xl font-semibold tracking-wide">
                    <h2 className="p-2 text-xl font-semibold">Console</h2>
                </div>

            </div>
        </div>
    );
}

