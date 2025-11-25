import React from "react";
import type { CandleData } from "../utils";

interface CandleInfoProps {
    candle: CandleData;
}

const CandleInfo: React.FC<CandleInfoProps> = ({ candle }) => {
    const diff = candle.close - candle.open;
    const pct = candle.open !== 0 ? (diff / candle.open) * 100 : 0;
    const diffClass =
        diff === 0
            ? "text-white/70"
            : diff > 0
              ? "text-green-400"
              : "text-red-400";

    return (
        <div className="pointer-events-none absolute right-4 top-3 rounded border border-white/20 bg-black/80 px-3 py-2 text-xs text-white/80 shadow-lg shadow-black/40">
            <div className="flex gap-2">
                <span className="text-white/50">O</span>
                <span>{candle.open.toFixed(2)}</span>
            </div>
            <div className="flex gap-2">
                <span className="text-white/50">H</span>
                <span>{candle.high.toFixed(2)}</span>
            </div>
            <div className="flex gap-2">
                <span className="text-white/50">L</span>
                <span>{candle.low.toFixed(2)}</span>
            </div>
            <div className="flex gap-2">
                <span className="text-white/50">C</span>
                <span>{candle.close.toFixed(2)}</span>
            </div>
            <div className="mt-1 flex justify-between text-[11px]">
                <span className="text-white/50">Δ</span>
                <span className={diffClass}>
                    {diff >= 0 ? "+" : ""}
                    {diff.toFixed(2)} ({pct >= 0 ? "+" : ""}
                    {pct.toFixed(2)}%)
                </span>
            </div>
        </div>
    );
};

export default CandleInfo;
