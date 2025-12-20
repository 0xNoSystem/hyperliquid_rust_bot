import React, { useMemo, useState, useCallback } from "react";
import { motion } from "framer-motion";
import { Pause, Play, Trash2, ExternalLink } from "lucide-react";
import type { MarketInfo } from "../types";
import {
    indicatorLabels,
    indicatorColors,
    decompose,
    get_value,
    get_params,
    fromTimeFrame,
    num,
    computeUPnL,
} from "../types";
import {MAX_DECIMALS} from "../consts";
import LoadingDots from "./Loading";
import { Link } from "react-router-dom";

interface MarketCardProps {
    market: MarketInfo;
    onTogglePause: (asset: string) => void;
    onRemove: (asset: string) => void;
}

const PnlBar: React.FC<{ pnl: number }> = ({ pnl }) => {
    const w = Math.min(100, Math.abs(pnl));
    const pos = pnl != null ? pnl >= 0 : true;
    return (
        <div className="rounded-md border border-white/10 bg-black p-1">
            <div className="h-1.5 w-full bg-white/5">
                <div
                    className={pos ? "bg-orange-400" : "bg-rose-500"}
                    style={{ width: `${w}%`, height: "100%" }}
                />
            </div>
            <div
                className={`mt-1 text-right font-mono text-[11px] tabular-nums ${
                    pos ? "text-orange-300" : "text-rose-300"
                }`}
            >
                {pos ? "+" : ""}
                {pnl != null ? pnl.toFixed(2) : 0.0}
            </div>
        </div>
    );
};

const MarketCard: React.FC<MarketCardProps> = ({
    market,
    assetMeta,
    onTogglePause,
    onRemove,
}) => {
    const {
        asset,
        state,
        price,
        prev,
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
    const pxDecimals = MAX_DECIMALS - szDecimals;
    const format = (n: number) => n.toFixed(pxDecimals);

    const price_color = "orange";

    const loading = state === "Loading";

    return (
        <motion.div
            whileHover={{ y: -2 }}
            className="group rounded-md border border-white/10 bg-[#111316] p-4 shadow-[0_2px_0_rgba(255,255,255,0.03),_0_12px_24px_rgba(0,0,0,0.35)] hover:bg-[#111311]"
        >
            {/* Head */}
            <div className="mb-3 flex items-start justify-between">
                <div>
                    <div className="text-[10px] text-white/50 uppercase">
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
                                className="ml-3 hidden items-center gap-2 rounded-md border border-white/10 bg-[#111316] text-[12px] text-white hover:bg-white/5 md:inline-flex"
                            >
                                <ExternalLink className="h-3.5 w-3.5 text-orange-400" />
                            </a>
                        </h2>
                        <span
                            className={`relative bottom-1 rounded-md px-2 py-0.5 text-[10px] uppercase ${
                                isPaused
                                    ? "border border-amber-400/60 text-amber-300"
                                    : "border border-orange-500/60 text-orange-300"
                            }`}
                        >
                            {loading ? "Loading" : isPaused ? "Paused" : "Live"}
                        </span>
                    </div>
                    <div className="mt-1 font-mono text-sm text-white/70">
                        {loading || lev == null ? <LoadingDots /> : `${lev}×`}
                    </div>
                </div>

                {loading ? null : (
                    <div className="flex gap-2">
                        <button
                            onClick={() => onTogglePause(asset)}
                            className="grid h-9 w-9 place-items-center rounded-md border border-white/10 bg-white/[0.04] hover:bg-white/10"
                            title="Toggle"
                        >
                            {isPaused ? (
                                <Play className="h-4 w-4 text-orange-300" />
                            ) : (
                                <Pause className="h-4 w-4 text-amber-300" />
                            )}
                        </button>
                        <button
                            onClick={() => onRemove(asset)}
                            className="grid h-9 w-9 place-items-center rounded-md border border-white/10 bg-white/[0.04] hover:bg-rose-600/20"
                            title="Remove"
                        >
                            <Trash2 className="h-4 w-4 text-rose-300" />
                        </button>
                    </div>
                )}
            </div>

            {/* Metrics */}
            <div className="grid grid-cols-3 gap-3">
                <div>
                    <div className="text-[10px] text-white/50 uppercase">
                        Price
                    </div>
                    <div
                        className={`font-mono text-xl font-semibold tabular-nums`}
                    >
                        <span className={`text-${price_color}-400 `}>
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
                    <div className="text-[10px] text-white/50 uppercase">
                        Leverage
                    </div>
                    <div className="font-mono text-xl">
                        {loading || lev == null ? <LoadingDots /> : `${lev}×`}
                    </div>
                </div>
                <div>
                    <div className="text-[10px] text-white/50 uppercase">
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
                <div className="text-[10px] text-white/50 uppercase">PnL</div>
                <PnlBar pnl={pnl} />
            </div>

            {/* Indicators */}
            <div className="mt-3 flex flex-wrap gap-2">
                {loading ? (
                    <LoadingDots />
                ) : (
                    indicators.map((data, i) => {
                        const { kind, timeframe, value } = decompose(data);
                        const kindKey = Object.keys(
                            kind
                        )[0] as keyof typeof indicatorColors;

                        return (
                            <div
                                className={`flex cursor-pointer flex-col rounded-md border border-white/10 px-2.5 py-1 text-[11px] ${indicatorColors[kindKey]}`}
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
            <div className="mt-4 border-t border-white/10 pt-3 text-xs">
                {loading ? (
                    <div className="col-span-3 flex justify-center">
                        <LoadingDots />
                    </div>
                ) : (
                    <>
                    <div className= "rounded-xl border border-white/10 bg-[#0B0E12]/80 my-2">
                    <p className="py-1 text-center">
                        OPEN POSITION
                    </p>

                    <div className="px-3 py-2">
                        {position == null ? (
                            <p className="text-center">---</p>
                        ) : (
                            <table className="min-w-full text-[11px]">
                                <thead className="text-white/60">
                                    <tr>
                                        <th className="py-2 pr-2 text-left">
                                            Side
                                        </th>

                                        <th className="py-2 pr-2 text-right">
                                            Entry
                                        </th>

                                        <th className="py-2 pr-2 text-right">
                                            Size
                                        </th>

                                        <th className="py-2 pr-2 text-right">
                                            Funding
                                        </th>

                                        <th className="py-2 text-right">
                                            UPNL
                                        </th>
                                    </tr>
                                </thead>

                                <tbody>
                                    <tr className="border-t border-white/10">
                                        <td
                                            className={`py-2 pr-4 font-semibold uppercase ${
                                                position.side === "long"
                                                    ? "text-green-500"
                                                    : "text-red-500"
                                            }`}
                                        >
                                            {position.side}
                                        </td>

                                        <td className="py-2 pr-2 text-right">
                                            {format(position.entryPx)}
                                        </td>

                                        <td className="py-2 pr-2 text-right">
                                            {num(
                                                position.size, szDecimals,
                                            )}
                                        </td>

                                        <td className="py-2 pr-2 text-right">
                                            {num(
                                                position.funding,2
                                            )}
                                            $
                                        </td>

                                        <td className="py-2 text-right text-orange-400">
                                            {num(
                                                computeUPnL(
                                                    position,
                                                    price
                                                ), 2
                                            )}
                                            $
                                        </td>
                                    </tr>
                                </tbody>
                            </table>
                        )}
                    </div>
                </div>

                        <div className="text-center">
                            <div className="text-[12px] text-white/50 uppercase">
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
