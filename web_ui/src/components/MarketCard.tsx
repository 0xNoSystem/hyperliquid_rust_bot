import { motion } from "framer-motion";

import { Pause, Play, Trash2, ExternalLink } from "lucide-react";
import type { MarketInfo, assetMeta, IndicatorName } from "../types";
import {
    indicatorLabels,
    indicatorColors,
    decompose,
    get_value,
    get_params,
    fromTimeFrame,
} from "../types";
import { MAX_DECIMALS } from "../consts";
import LoadingDots from "./Loading";
import { Link } from "react-router-dom";
import PositionTable from "./Position";

interface MarketCardProps {
    market: MarketInfo;
    assetMeta?: assetMeta;
    onTogglePause: (asset: string) => void;
    onRemove: (asset: string) => void;
}

const PnlBar = ({ pnl }: { pnl: number | null }) => {
    const safePnl = pnl ?? 0;
    const w = Math.min(100, Math.abs(safePnl));
    const pos = safePnl >= 0;
    return (
        <div className="border-pnl-shell-border bg-pnl-shell-bg rounded-md border p-1">
            <div className="bg-pnl-track h-1.5 w-full">
                <div
                    className={
                        pos ? "bg-pnl-positive-bg" : "bg-pnl-negative-bg"
                    }
                    style={{ width: `${w}%`, height: "100%" }}
                />
            </div>
            <div
                className={`mt-1 text-right font-mono text-[11px] tabular-nums ${
                    pos ? "text-pnl-positive-text" : "text-pnl-negative-text"
                }`}
            >
                {pos ? "+" : ""}
                {safePnl.toFixed(2)}
            </div>
        </div>
    );
};

const MarketCard = ({
    market,
    assetMeta,
    onTogglePause,
    onRemove,
}: MarketCardProps) => {
    const {
        asset,
        state,
        price,
        lev,
        margin,
        params,
        pnl,
        isPaused,
        indicators,
        position,
    } = market;
    const { strategy } = params;
    const szDecimals = assetMeta ? assetMeta.szDecimals : 3;
    const pxDecimals = MAX_DECIMALS - szDecimals - 1;
    const format = (n: number) => n.toFixed(pxDecimals);

    const loading = state === "Loading";

    return (
        <motion.div
            whileHover={{ y: -2 }}
            className="group border-accent-brand/10 bg-app-surface-2 hover:bg-app-surface-hover rounded-md border p-4"
        >
            {/* Head */}
            <div className="mb-3 flex items-start justify-between">
                <div>
                    <div className="text-app-text/50 text-[10px] uppercase">
                        Asset
                    </div>
                    <div className="-mt-0.5 flex items-baseline gap-3">
                        <h2 className="text-3xl font-semibold tracking-tight">
                            {loading ? (
                                asset
                            ) : (
                                <Link
                                    to={`/asset/${asset}`}
                                    className="hover:underline"
                                >
                                    {asset}
                                </Link>
                            )}
                            <a
                                href={`https://app.hyperliquid.xyz/trade/${asset}`}
                                target="_blank"
                                rel="noopener noreferrer"
                                className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 ml-3 hidden items-center gap-2 rounded-md border text-[12px] md:inline-flex"
                            >
                                <ExternalLink className="text-accent-brand h-3.5 w-3.5" />
                            </a>
                        </h2>
                        <span
                            className={`relative bottom-1 rounded-md px-2 py-0.5 text-[10px] uppercase ${
                                isPaused
                                    ? "border-accent-warning-border/60 text-accent-warning-mid border"
                                    : "border-accent-brand-strong/60 text-accent-brand-soft border"
                            }`}
                        >
                            {loading ? "Loading" : isPaused ? "Paused" : "Live"}
                        </span>
                    </div>
                    <div className="text-app-text/70 mt-1 font-mono text-sm">
                        {loading || lev == null ? <LoadingDots /> : `${lev}×`}
                    </div>
                </div>

                {loading ? null : (
                    <div className="flex gap-2">
                        <button
                            onClick={() => onTogglePause(asset)}
                            className="border-line-subtle bg-glow-4 hover:bg-glow-10 grid h-9 w-9 place-items-center rounded-md border"
                            title="Toggle"
                        >
                            {isPaused ? (
                                <Play className="text-accent-brand-soft h-4 w-4" />
                            ) : (
                                <Pause className="text-accent-warning-mid h-4 w-4" />
                            )}
                        </button>
                        <button
                            onClick={() => onRemove(asset)}
                            className="border-line-subtle bg-glow-4 hover:bg-accent-danger-alt-strong/20 grid h-9 w-9 place-items-center rounded-md border"
                            title="Remove"
                        >
                            <Trash2 className="text-accent-danger-alt-soft h-4 w-4" />
                        </button>
                    </div>
                )}
            </div>

            {/* Metrics */}
            <div className="grid grid-cols-3 gap-3">
                <div>
                    <div className="text-app-text/50 text-[10px] uppercase">
                        Price
                    </div>
                    <div
                        className={`font-mono text-xl font-semibold tabular-nums`}
                    >
                        <span className="text-accent-brand">
                            {loading ||
                            price == null ||
                            Math.abs(price) < 1e-8 ? (
                                <LoadingDots />
                            ) : (
                                `$${format(price)}`
                            )}
                        </span>
                    </div>
                </div>
                <div>
                    <div className="text-app-text/50 text-[10px] uppercase">
                        Leverage
                    </div>
                    <div className="font-mono text-xl">
                        {loading || lev == null ? <LoadingDots /> : `${lev}×`}
                    </div>
                </div>
                <div>
                    <div className="text-app-text/50 text-[10px] uppercase">
                        Margin
                    </div>
                    <div className="font-mono text-xl">
                        {loading || margin == null ? (
                            <LoadingDots />
                        ) : (
                            `$${margin.toFixed(2)}`
                        )}
                    </div>
                </div>
            </div>

            {/* PnL */}
            <div className="mt-4">
                <div className="text-app-text/50 text-[10px] uppercase">
                    PnL
                </div>
                <PnlBar pnl={pnl} />
            </div>

            {/* Indicators */}
            <div className="mt-3 flex flex-wrap gap-2">
                {loading ? (
                    <LoadingDots />
                ) : (
                    indicators.map((data, i) => {
                        const { kind, timeframe, value } = decompose(data);
                        const kindKey = Object.keys(kind)[0] as IndicatorName;

                        return (
                            <div
                                className={`border-line-subtle flex cursor-pointer flex-col rounded-md border px-2.5 py-1 text-[11px] ${indicatorColors[kindKey]}`}
                                title={get_params(kind)}
                            >
                                <span key={i}>
                                    {indicatorLabels[kindKey] ||
                                        (kindKey as string)}{" "}
                                    — {fromTimeFrame(timeframe)}
                                </span>
                                <span className="text-center text-base font-bold">
                                    {value
                                        ? get_value(value, pxDecimals)
                                        : "N/A"}
                                </span>
                            </div>
                        );
                    })
                )}
            </div>

            {/* Strategy */}
            <div className="border-line-subtle mt-4 border-t pt-3 text-xs">
                {loading ? (
                    <div className="col-span-3 flex justify-center">
                        <LoadingDots />
                    </div>
                ) : (
                    <>
                        <div className="border-line-subtle bg-surface-pane my-2 rounded-xl border">
                            <p className="py-1 text-center">OPEN POSITION</p>

                            <div className="px-3 py-2">
                                {position == null ? (
                                    <p className="text-center">---</p>
                                ) : (
                                    <PositionTable
                                        position={position}
                                        price={price}
                                        lev={lev}
                                        szDecimals={szDecimals}
                                        formatPrice={format}
                                    />
                                )}
                            </div>
                        </div>

                        <div className="text-center">
                            <div className="text-app-text/50 text-[12px] uppercase">
                                Strategy
                            </div>
                            <p className="text-[14px] font-bold">{strategy}</p>
                        </div>
                    </>
                )}
            </div>
        </motion.div>
    );
};

export default MarketCard;
