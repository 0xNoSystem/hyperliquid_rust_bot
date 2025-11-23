import React, { useState } from "react";
import { useParams } from "react-router-dom";
import { scaleTime, scaleLinear } from "@visx/scale";
import { LinePath } from "@visx/shape";
import { into, TIMEFRAME_CAMELCASE } from "../types";
import type { TimeFrame } from "../types";
import Chart from "../chart/Chart";

export default function Backtest({ data, width = 800, height = 300 }) {
    const { asset: routeAsset } = useParams<{ asset: string }>();
    const [timeframe, setTimeframe] = useState<TimeFrame>("min1");
    const [intervalOn, setIntervalOn] = useState(false);

    return (
        <div className="flex h-full flex-col bg-black/70 pb-50">
            {/* Title */}
            <h1 className="mt-6 p-2 text-center text-3xl font-bold tracking-widest">
                STRATEGY LAB
            </h1>

            {/* Column Layout */}
            <div className="z-1 flex flex-grow flex-col items-center justify-between py-8">
                {/* Strategy (Top) */}{" "}
                <div className="mb-6 mb-30 w-[60%] border-2 border-white/70 bg-black/60 p-4 text-center tracking-widest">
                    <h2 className="p-2 text-xl font-semibold">Strategy</h2>
                </div>
                {/* Chart (Middle) */}
                <div className="mb-6 mb-30 flex min-h-[70vh] w-[90%] flex-grow flex-col rounded-lg border-2 border-white/70 bg-gray-500/30 p-4 tracking-widest">
                    <div>
                        <div className="p-4 pl-1 flex">
                        <button
                            onClick={() => setIntervalOn(!intervalOn)}
                            className={`relative flex h-6 w-12 mr-5 items-center rounded-full cursor-pointer transition-colors duration-300 ${intervalOn ? "bg-orange-500" : "bg-gray-600"} `}
                        >
                            <span
                                className={`absolute top-1 left-1 h-4 w-4 rounded-full bg-white transition-transform duration-300 ${intervalOn ? "translate-x-6" : "translate-x-0"} `}
                            />
                        </button>
                            <h3 className="tracking-wide">Select BT period {intervalOn ? "On" : "Off"}</h3>
                        </div>
                        <h2 className="rounded-t-lg bg-black/80 p-2 text-center text-2xl font-semibold">
                            {routeAsset}
                        </h2>
                    </div>

                    <div className="flex flex-1 flex-col rounded-b-lg border-2 border-black/30 bg-[#111212]">
                        <div className="z-5 grid w-full grid-cols-13 bg-black/70 tracking-normal text-center">
                            {Object.entries(TIMEFRAME_CAMELCASE).map(
                                ([short, tf]) => (
                                    <div
                                        className="border-b-2 border-black py-2 text-white/70 hover:bg-black"
                                        key={short}
                                        onClick={() => setTimeframe(tf)}
                                    >
                                        <span
                                            className={`cursor-default px-2 text-center text-sm ${
                                                timeframe === tf
                                                    ? "font-bold text-orange-500 hover:text-orange-500"
                                                    : ""
                                            }`}
                                        >
                                            {short}
                                        </span>
                                    </div>
                                )
                            )}
                        </div>

                        <Chart asset={routeAsset} timeframe={timeframe} settingInterval={intervalOn}/>
                    </div>
                </div>
                {/* Console (Bottom) */}
                <div className="w-[60%] border-2 border-white/70 bg-black/60 p-2 text-center text-xl font-semibold tracking-wide">
                    <h2 className="p-2 text-xl font-semibold">Console</h2>
                </div>
            </div>
        </div>
    );
}
