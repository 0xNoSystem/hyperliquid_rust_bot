import React from "react";
import type { AddMarketInfo, IndicatorKind} from "../types";
import { indicatorColors,decompose, indicatorLabels, get_params} from "../types";

interface CachedMarketProps {
  market: AddMarketInfo;
  onAdd: (asset: string) => void;
  onRemove: (asset:string) => void;
}

export const CachedMarket: React.FC<CachedMarketProps> = ({ market, onAdd, onRemove }) => {
  const { asset, marginAlloc, tradeParams, config } = market;

  return (
    <div className="bg-gray-300/30 my-2 text-black flex items-center justify-between border-2 rounded-lg border-gray-600 py-1 px-3 hover:bg-gray-300 h-fit">
      <div className="flex items-center space-x-6 text-sm h-full">
        <span className="flex items-center justify-center font-medium w-20 bg-orange-300/50 rounded-lg h-full text-center">{asset}</span>
        <span className="w-max">
          Margin: {"alloc" in marginAlloc ? marginAlloc.alloc : marginAlloc.amount.toFixed(2)}$
        </span>
        <span className="w-24">Lev: {tradeParams.lev}x</span>
        <div className="flex flex-col">
            {config.map(([ind, tf], i) => {
              const kind = Object.keys(ind)[0] as IndicatorKind;
              return (
                <div key={i} className="flex items-center ml-2 mb-3">
                  <span className={`${indicatorColors[kind]} px-3 py-1 rounded-full text-xs`}>{indicatorLabels[kind] || kind} -- {tf}</span>
                </div>
              );
            })}
            </div>
        <span className="flex flex-col">
            <span><strong className="ml-4">Risk:</strong> {tradeParams.strategy.custom.risk}</span>
            <span><strong className="ml-4">Stance:</strong> {tradeParams.strategy.custom.stance}</span>
            <span><strong className="ml-4">Style:</strong> {tradeParams.strategy.custom.style}</span>
            <span><strong className="ml-4">Trend Follow:</strong> {tradeParams.strategy.custom.followTrend ? "Yes" : "No"}</span>
        </span>
      </div>
      <div className="flex">
    <button
        variant="outline"
        className="bg-red-500/30 py-3 px-3 rounded-lg hover:bg-red-500 hover:cursor-pointer"
        onClick={() => onRemove(asset)}
      >
      <strong>X</strong>
      </button>

      <button
        variant="outline"
        className="ml-2 bg-cyan-300/30 py-3 px-4 rounded-lg hover:bg-cyan-700 hover:cursor-pointer"
        onClick={() => onAdd(asset)}
      >
        Add
      </button>
      </div>
    </div>
  );
};





