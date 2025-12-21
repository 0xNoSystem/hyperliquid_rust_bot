import React from "react";
import type { AddMarketInfo, IndicatorName } from "../types";
import { indicatorColors, indicatorLabels } from "../types";

interface CachedMarketProps {
    market: AddMarketInfo;
    onAdd: (asset: string) => void;
    onRemove: (asset: string) => void;
}

export const CachedMarket: React.FC<CachedMarketProps> = ({
    market,
    onAdd,
    onRemove,
}) => {
    const { asset, marginAlloc, tradeParams, config } = market;

    return (
        <div className="my-2 flex h-fit items-center justify-between rounded-lg border-2 border-gray-600 bg-gray-300/30 px-3 py-1 text-black hover:bg-gray-300">
            <div className="flex h-full items-center space-x-6 text-sm">
                <span className="flex h-full w-20 items-center justify-center rounded-lg bg-orange-300/50 text-center font-medium">
                    {asset}
                </span>
                <span className="w-max">
                    Margin:{" "}
                    {"alloc" in marginAlloc
                        ? marginAlloc.alloc
                        : marginAlloc.amount.toFixed(2)}
                    $
                </span>
                <span className="w-24">Lev: {tradeParams.lev}x</span>
                <div className="flex flex-col">
                    {(config ?? []).map(([ind, tf], i) => {
                        const kind = Object.keys(ind)[0] as IndicatorName;
                        return (
                            <div
                                key={i}
                                className="mb-3 ml-2 flex items-center"
                            >
                                <span
                                    className={`${indicatorColors[kind]} rounded-full px-3 py-1 text-xs`}
                                >
                                    {indicatorLabels[kind] || kind} -- {tf}
                                </span>
                            </div>
                        );
                    })}
                </div>
                <span className="flex flex-col">{tradeParams.strategy}</span>
            </div>
            <div className="flex">
                <button
                    className="rounded-lg bg-red-500/30 px-3 py-3 hover:cursor-pointer hover:bg-red-500"
                    onClick={() => onRemove(asset)}
                >
                    <strong>X</strong>
                </button>

                <button
                    className="ml-2 rounded-lg bg-cyan-300/30 px-4 py-3 hover:cursor-pointer hover:bg-cyan-700"
                    onClick={() => onAdd(asset)}
                >
                    Add
                </button>
            </div>
        </div>
    );
};
