import React from "react";
import { formatVolume, type CandleData } from "../utils";

interface CandleInfoProps {
    candle: CandleData;
}

const formatPrice = (n: number) => {
    if (n > 1 && n < 2) return n.toFixed(4);
    if (n < 1) return n.toFixed(6);
    return n.toFixed(2);
};

const CandleInfo: React.FC<CandleInfoProps> = ({ candle }) => {
    const diff = candle.close - candle.open;
    const pct = candle.open !== 0 ? (diff / candle.open) * 100 : 0;
    const diffClass =
        diff === 0
            ? "text-app-text/70"
            : diff > 0
              ? "text-accent-success"
              : "text-accent-danger-soft";

    return (
        <div className="border-line-weak bg-app-surface-1 text-app-text/80 shadow-app-ink/20 pointer-events-none absolute top-3 left-4 rounded border px-3 py-2 text-xs shadow-sm">
            <div className="flex gap-2">
                <span className="text-app-text/50">H</span>
                <span>{formatPrice(candle.high)}</span>
            </div>
            <div className="flex gap-2">
                <span className="text-app-text/50">C</span>
                <span>{formatPrice(candle.close)}</span>
            </div>
            <div className="flex gap-2">
                <span className="text-app-text/50">L</span>
                <span>{formatPrice(candle.low)}</span>
            </div>
            <div className="flex gap-2">
                <span className="text-app-text/50">O</span>
                <span>{formatPrice(candle.open)}</span>
            </div>
            <div className="flex gap-2">
                <span className="text-app-text/50">VLM</span>
                <span>{formatVolume(candle.volume)}</span>
            </div>

            <div className="mt-1 flex justify-between text-[11px]">
                <span className="text-app-text/50">Δ</span>
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
