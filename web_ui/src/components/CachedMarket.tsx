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
        <div className="my-2 flex h-fit items-center justify-between rounded-lg border-2 border-line-ink-strong bg-surface-input-soft px-3 py-1 font-semibold text-ink hover:bg-surface-input-muted">
            <div className="jusify-center flex h-full items-center space-x-12 text-sm">
                <span className="flex h-full w-20 items-center justify-center rounded-lg bg-ink-50 text-center font-medium text-app-text/80">
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
                <div className="ml-10 flex items-center gap-2 text-[13px] font-bold text-ink-muted">
                    <span className="tracking-wide uppercase">Strategy</span>
                    <span className="rounded-md bg-ink-50 px-2 py-1 text-[12px] font-semibold text-app-text/70">
                        {tradeParams.strategy}
                    </span>
                </div>
            </div>
            <div className="flex">
                <button
                    className="rounded-lg bg-glow-50 px-3 py-3 hover:cursor-pointer hover:bg-ink-70 hover:text-app-text"
                    onClick={() => onRemove(asset)}
                >
                    <strong>X</strong>
                </button>

                <button
                    className="ml-2 rounded-lg bg-glow-50 px-4 py-3 hover:cursor-pointer hover:bg-ink-50 hover:text-accent-brand"
                    onClick={() => onAdd(asset)}
                >
                    Add
                </button>
            </div>
        </div>
    );
};
