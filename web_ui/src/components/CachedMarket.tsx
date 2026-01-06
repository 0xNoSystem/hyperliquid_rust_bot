import React from "react";
import type { AddMarketInfo, IndicatorName } from "../types";
import { indicatorColors, get_params, indicatorLabels } from "../types";

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
        <div className="border-line-ink-strong bg-surface-input-soft/80 text-ink hover:bg-surface-input-soft my-2 flex h-fit min-w-fit items-center justify-between rounded-lg border-2 px-3 py-1 font-semibold">
            <div className="jusify-center flex h-full items-center space-x-12 text-sm">
                <span className="bg-ink-50 text-app-text/80 flex h-full w-20 items-center justify-center rounded-lg text-center font-medium">
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
                <div className="flex flex-col font-normal">
                    {(config ?? []).map(([ind, tf], i) => {
                        const kind = Object.keys(ind)[0] as IndicatorName;
                        return (
                            <div
                                key={i}
                                title={get_params(ind)}
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
                <div className="text-ink-muted ml-10 flex items-center gap-2 text-[13px] font-bold">
                    <span className="tracking-wide uppercase">Strategy</span>
                    <span className="bg-ink-50 text-app-text/70 rounded-md px-2 py-1 text-[12px] font-semibold">
                        {tradeParams.strategy}
                    </span>
                </div>
            </div>
            <div className="flex">
                <button
                    className="bg-glow-50 hover:bg-ink-70 hover:text-app-text rounded-lg px-3 py-3 hover:cursor-pointer"
                    onClick={() => onRemove(asset)}
                >
                    <strong>X</strong>
                </button>

                <button
                    className="bg-glow-50 hover:bg-ink-50 hover:text-accent-brand ml-2 rounded-lg px-4 py-3 hover:cursor-pointer"
                    onClick={() => onAdd(asset)}
                >
                    Add
                </button>
            </div>
        </div>
    );
};
