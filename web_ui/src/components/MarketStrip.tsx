import { Pause, Play, Trash2, ExternalLink } from "lucide-react";
import { Link } from "react-router-dom";
import Spinner from "./Spinner";
import type { MarketInfo, assetMeta } from "../types";
import {
    indicatorLabels,
    indicatorColors,
    indicator_name,
    decompose,
    get_value,
    get_params,
    fromTimeFrame,
    computeUPnL,
    num,
    engineDisplayLabel,
} from "../types";
import { MAX_DECIMALS } from "../consts";
import LoadingDots from "./Loading";

interface MarketStripProps {
    market: MarketInfo;
    assetMeta?: assetMeta;
    onTogglePause: (asset: string) => void;
    onRemove: (asset: string) => void;
    isToggling?: boolean;
}

const MarketStrip = ({
    market,
    assetMeta,
    onTogglePause,
    onRemove,
    isToggling,
}: MarketStripProps) => {
    const {
        asset,
        state,
        price,
        lev,
        margin,
        strategyName,
        pnl,
        isPaused,
        indicators,
        engineState,
        position,
    } = market;
    const szDecimals = assetMeta ? assetMeta.szDecimals : 3;
    const pxDecimals = MAX_DECIMALS - szDecimals - 1;
    const format = (n: number) => n.toFixed(pxDecimals);
    const loading = state === "Loading";
    const positivePnl = (pnl ?? 0) >= 0;
    const positionPnl =
        position && price != null && lev != null
            ? computeUPnL(position, price, lev)
            : null;

    return (
        <div className="border-accent-brand/10 bg-app-surface-1/55 hover:border-accent-brand/25 hover:bg-app-surface-1 group relative z-0 flex min-h-12 flex-wrap items-center gap-x-2 gap-y-1 overflow-visible rounded-md border px-2 py-1.5 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)] transition-colors hover:z-50 sm:flex-nowrap sm:gap-3 sm:px-3 sm:py-2">
            <div className="flex min-w-0 flex-1 items-center gap-1.5 sm:min-w-36 sm:flex-none sm:shrink-0 sm:gap-2">
                <div className="min-w-0">
                    <div className="flex items-center gap-1.5 sm:gap-2">
                        <h2 className="truncate text-sm font-semibold tracking-tight sm:text-base">
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
                        </h2>
                        <span
                            className={`hidden shrink-0 rounded-full px-2 py-0.5 text-[10px] uppercase sm:inline-flex ${
                                isPaused
                                    ? "border-accent-warning-border/60 text-accent-warning-mid border"
                                    : "border-accent-brand-strong/60 text-accent-brand-soft border"
                            }`}
                        >
                            {loading ? "Loading" : isPaused ? "Paused" : "Live"}
                        </span>
                    </div>
                    <div className="text-app-text/45 font-mono text-[10px] sm:text-[11px]">
                        {loading || lev == null ? <LoadingDots /> : `${lev}x`}
                    </div>
                </div>
                {!loading && (
                    <a
                        href={`https://app.hyperliquid.xyz/trade/${asset}`}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-app-text/30 hover:text-accent-brand shrink-0 transition-colors"
                    >
                        <ExternalLink className="h-3.5 w-3.5" />
                    </a>
                )}
            </div>

            <div className="bg-line-subtle/70 hidden h-7 w-px shrink-0 sm:block" />

            <div className="min-w-0 shrink-0">
                <div className="text-app-text/40 hidden text-[9px] tracking-wide uppercase sm:block">
                    Price
                </div>
                <div className="text-accent-brand font-mono text-xs font-semibold tabular-nums sm:text-sm">
                    {loading || price == null || Math.abs(price) < 1e-8 ? (
                        <LoadingDots />
                    ) : (
                        `$${format(price)}`
                    )}
                </div>
            </div>

            <div className="min-w-0 shrink-0">
                <div className="text-app-text/40 hidden text-[9px] tracking-wide uppercase sm:block">
                    Margin
                </div>
                <div className="font-mono text-xs tabular-nums sm:text-sm">
                    {loading || margin == null ? (
                        <LoadingDots />
                    ) : (
                        `$${margin.toFixed(2)}`
                    )}
                </div>
            </div>

            <div className="min-w-0 shrink-0">
                <div className="text-app-text/40 hidden text-[9px] tracking-wide uppercase sm:block">
                    PnL
                </div>
                <div
                    className={`font-mono text-xs tabular-nums sm:text-sm ${
                        positivePnl
                            ? "text-pnl-positive-text"
                            : "text-pnl-negative-text"
                    }`}
                >
                    {loading || pnl == null ? (
                        <LoadingDots />
                    ) : (
                        `${positivePnl ? "+" : ""}${pnl.toFixed(2)}`
                    )}
                </div>
            </div>

            <div className="max-w-24 min-w-0 shrink truncate sm:max-w-none sm:min-w-32 sm:shrink-0">
                <div className="text-app-text/40 hidden text-[9px] tracking-wide uppercase sm:block">
                    Strategy
                </div>
                <div className="truncate text-[11px] font-medium sm:text-xs">
                    {loading ? <LoadingDots /> : strategyName || "—"}
                </div>
            </div>

            <div className="shrink-0">
                <div className="group/indicators relative">
                    <button className="border-line-subtle bg-glow-4 hover:bg-glow-10 text-app-text rounded-full border px-2 py-1 text-[11px] sm:px-3 sm:py-1.5 sm:text-xs">
                        {loading ? (
                            <LoadingDots />
                        ) : (
                            `${indicators.length} ${
                                indicators.length === 1
                                    ? "Indicator"
                                    : "Indicators"
                            }`
                        )}
                    </button>
                    {!loading && (
                        <div className="border-line-subtle bg-surface-pane invisible absolute bottom-full left-0 z-[999] mb-2 w-72 max-w-[calc(100vw-2rem)] rounded-md border p-3 opacity-0 shadow-xl transition group-hover/indicators:visible group-hover/indicators:opacity-100 sm:right-0 sm:left-auto">
                            <div className="text-app-text mb-2 text-[10px] uppercase">
                                Active Indicators
                            </div>
                            {indicators.length === 0 ? (
                                <p className="text-app-text text-xs">
                                    No indicators configured.
                                </p>
                            ) : (
                                <div className="flex max-h-80 flex-wrap gap-2 overflow-y-auto">
                                    {indicators.map((data, i) => {
                                        const {
                                            asset,
                                            kind,
                                            timeframe,
                                            value,
                                        } = decompose(data);
                                        const kindKey = indicator_name(kind);

                                        return (
                                            <div
                                                key={`${asset}-${kindKey}-${fromTimeFrame(timeframe)}-${i}`}
                                                className={`border-line-subtle flex cursor-pointer flex-col rounded-md border px-2.5 py-1 text-[11px] ${indicatorColors[kindKey]}`}
                                                title={get_params(kind)}
                                            >
                                                <span className="text-center font-bold">
                                                    {asset}
                                                </span>
                                                <span>
                                                    {indicatorLabels[kindKey] ||
                                                        (kindKey as string)}{" "}
                                                    - {fromTimeFrame(timeframe)}
                                                </span>
                                                <span className="text-center text-base font-bold">
                                                    {value
                                                        ? get_value(
                                                              value,
                                                              pxDecimals
                                                          )
                                                        : "N/A"}
                                                </span>
                                            </div>
                                        );
                                    })}
                                </div>
                            )}
                        </div>
                    )}
                </div>
            </div>

            <div className="max-w-36 min-w-0 shrink-0 truncate">
                <div className="text-app-text/40 hidden text-[9px] tracking-wide uppercase sm:block">
                    Engine
                </div>
                <div className="truncate text-[11px] font-semibold text-orange-500 sm:text-xs">
                    {loading ? (
                        <LoadingDots />
                    ) : (
                        engineDisplayLabel(engineState, position)
                    )}
                </div>
            </div>

            <div className="max-w-36 min-w-0 shrink truncate sm:max-w-none sm:min-w-44 sm:shrink-0">
                <div className="text-app-text/40 hidden text-[9px] tracking-wide uppercase sm:block">
                    Position
                </div>
                {loading ? (
                    <LoadingDots />
                ) : position == null ? (
                    <div className="text-app-text/35 text-[11px] sm:text-xs">
                        No position
                    </div>
                ) : (
                    <div className="flex items-center gap-1.5 truncate text-[11px] sm:gap-2 sm:text-xs">
                        <span
                            className={`font-semibold uppercase ${
                                position.side === "long"
                                    ? "text-accent-success-strong"
                                    : "text-accent-danger"
                            }`}
                        >
                            {position.side}
                        </span>
                        <span className="font-mono tabular-nums">
                            {num(position.size, szDecimals)} @{" "}
                            {format(position.entryPx)}
                        </span>
                        {positionPnl && (
                            <span
                                className={`font-mono tabular-nums ${
                                    positionPnl[0] >= 0
                                        ? "text-pnl-positive-text"
                                        : "text-pnl-negative-text"
                                }`}
                            >
                                {num(positionPnl[0], 2)}$
                            </span>
                        )}
                    </div>
                )}
            </div>

            <div className="ml-auto flex shrink-0 items-center gap-1.5 sm:gap-2">
                {loading ? null : (
                    <>
                        <button
                            onClick={() => onTogglePause(asset)}
                            disabled={isToggling}
                            className="border-line-subtle bg-glow-4 hover:bg-glow-10 grid h-7 w-7 place-items-center rounded-full border disabled:opacity-50 sm:h-8 sm:w-8"
                            title="Toggle"
                        >
                            {isToggling ? (
                                <Spinner className="text-app-text/50 h-4 w-4" />
                            ) : isPaused ? (
                                <Play className="text-accent-brand-soft h-4 w-4" />
                            ) : (
                                <Pause className="text-accent-warning-mid h-4 w-4" />
                            )}
                        </button>
                        <button
                            onClick={() => onRemove(asset)}
                            className="border-line-subtle bg-glow-4 hover:bg-accent-danger-alt-strong/20 grid h-7 w-7 place-items-center rounded-full border sm:h-8 sm:w-8"
                            title="Remove"
                        >
                            <Trash2 className="text-accent-danger-alt-soft h-4 w-4" />
                        </button>
                    </>
                )}
            </div>
        </div>
    );
};

export default MarketStrip;
