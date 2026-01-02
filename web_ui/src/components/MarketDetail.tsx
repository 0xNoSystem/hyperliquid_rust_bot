// src/components/MarketDetail.tsx
// Alternative “Trading Terminal” layout — keyboard/terminal vibes, split panes, neon accents.
// Keeps the same backend interactions and batching behavior.
import { KwantChart } from "kwant";
import { useMemo, useState, useCallback, useEffect } from "react";
import { Link, useParams } from "react-router-dom";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { motion, AnimatePresence } from "framer-motion";
import { formatUTC } from "../chart/utils";
import { MAX_DECIMALS, MIN_ORDER_VALUE } from "../consts";
import { ErrorBanner } from "./ErrorBanner";
import PositionTable from "./Position";

import {
    decompose,
    indicatorLabels,
    indicatorParamLabels,
    indicatorColors,
    indicatorValueColors,
    fromTimeFrame,
    get_value,
    into,
    sanitizeAsset,
    num,
} from "../types";
import type {
    IndicatorKind,
    IndicatorName,
    IndexId,
    MarketInfo,
    TimeFrame,
    TradeInfo,
} from "../types";
import type { Strategy } from "../strats.ts";
import { strategyOptions } from "../strats.ts";
import { ArrowLeft, Plus, Minus, X } from "lucide-react";

const formatPrice = (n: number) => {
    if (n > 1 && n < 2) return n.toFixed(4);
    if (n < 1) return n.toFixed(6);
    return n.toFixed(2);
};

function leverageColor(lev: number, maxLev: number): string {
    const pct = (lev / maxLev) * 100;

    if (pct < 10) return "text-leverage-low";
    if (pct < 40) return "text-leverage-mid";
    if (pct < 60) return "text-leverage-high";
    if (pct < 80) return "text-leverage-critical";
    return "text-leverage-max";
}

/* ====== TOKENS ====== */
const Rail =
    "rounded-xl border border-line-subtle bg-surface-rail p-4 backdrop-blur";
const Pane = "rounded-xl border border-line-subtle bg-surface-pane";
const Head =
    "px-4 py-3 border-b border-line-subtle text-[11px] uppercase tracking-wide text-app-text/60";
const Body = "p-4";
const Chart = "";
const Input =
    "w-full rounded-lg px-3 py-2 border border-line-subtle bg-app-surface-3 text-app-text focus:outline-none focus:ring-2 focus:ring-accent-profit-strong/30";
const Select =
    "appearance-none w-full rounded-lg px-3 py-2 border border-line-subtle bg-app-surface-3 text-app-text focus:outline-none focus:ring-2 focus:ring-accent-profit-deep/30 ";
const BtnGhost =
    "inline-flex items-center justify-center rounded-lg border border-btn-ghost-border bg-btn-ghost-bg px-3 py-2 text-btn-ghost-text hover:bg-btn-ghost-hover";
const BtnOK =
    "inline-flex items-center justify-center rounded-lg border border-btn-ok-border bg-btn-ok-bg px-3 py-2 text-btn-ok-text hover:cursor-pointer hover:border-btn-ok-hover-border hover:bg-btn-ok-hover-bg hover:text-btn-ok-hover-text";
const Chip =
    "inline-flex items-center gap-2 rounded-md border border-btn-chip-border bg-btn-chip-bg px-2 py-1 text-[15px] text-btn-chip-text hover:cursor-pointer hover:bg-btn-chip-hover";
const GridCols =
    "grid grid-cols-1 xl:grid-cols-[300px_minmax(0,1fr)_360px] gap-4 p-8 ";

function PnlTicker({ pnl }: { pnl: number | null }) {
    if (pnl == null)
        return <span className="text-app-text/60 font-mono">PnL —</span>;
    const pos = pnl >= 0;
    return (
        <span
            className={`font-mono text-xl tabular-nums ${
                pos ? "text-accent-profit" : "text-accent-danger-alt-soft"
            }`}
        >
            {pos ? "+ $" : ""}
            {num(pnl, 2)}
        </span>
    );
}

type PendingEdit = { id: IndexId; edit: "add" | "remove" };
const kindKeys = Object.keys(indicatorParamLabels) as IndicatorName[];

export default function MarketDetail() {
    const { asset: routeAsset } = useParams<{ asset: string }>();
    const {
        markets,
        universe,
        sendCommand,
        requestToggleMarket,
        totalMargin,
        errorMsg,
        dismissError,
        updateMarketStrategy,
    } = useWebSocketContext();
    const [marketToToggle, setMarketToToggle] = useState<string | null>(null);

    const handleConfirmToggle = (asset: string, isPaused: boolean) => {
        if (isPaused) {
            requestToggleMarket(asset, false).catch((err) =>
                console.error("Toggle failed", err)
            );
        } else {
            setMarketToToggle(asset);
        }
    };

    const handleTogglePause = (asset: string) => {
        requestToggleMarket(asset, true)
            .catch((err) => console.error("Toggle failed", err))
            .finally(() => setMarketToToggle(null));
    };

    const market = useMemo<MarketInfo | undefined>(
        () => markets.find((m) => m.asset === (routeAsset ?? "")),
        [markets, routeAsset]
    );
    const meta = useMemo(
        () => universe.find((u) => u.name === market?.asset),
        [universe, market]
    );

    const pxDecimals = meta ? MAX_DECIMALS - meta.szDecimals - 1 : 3;

    /* ----- local state ----- */
    const [lev, setLev] = useState<number>(market?.lev ?? 1);
    const [margin, setMargin] = useState<number>(market?.margin ?? 0);

    // builder
    const [kindKey, setKindKey] = useState<IndicatorName>("rsi");
    const [p1, setP1] = useState<number>(14);
    const [p2, setP2] = useState<number>(14);
    const [tfSym, setTfSym] = useState<string>("1m");

    // batch
    const [pending, setPending] = useState<PendingEdit[]>([]);
    const [pendingStrategy, setPendingStrategy] = useState<Strategy | null>(
        null
    );

    const maxLev = meta?.maxLeverage ?? 1;
    const eqIndexId = (a: IndexId, b: IndexId) =>
        JSON.stringify(a) === JSON.stringify(b);

    const sendMarketCmd = (asset: string, cmd: unknown) =>
        sendCommand({ marketComm: { asset: asset, cmd } });

    const buildKind = useCallback((): IndicatorKind => {
        switch (kindKey) {
            case "emaCross":
                return { emaCross: { short: p1, long: p2 } };
            case "smaOnRsi":
                return { smaOnRsi: { periods: p1, smoothing_length: p2 } };
            case "stochRsi":
                return {
                    stochRsi: {
                        periods: p1,
                        k_smoothing: null,
                        d_smoothing: null,
                    },
                };
            case "adx":
                return { adx: { periods: p1, di_length: p2 } };
            case "rsi":
                return { rsi: p1 };
            case "atr":
                return { atr: p1 };
            case "ema":
                return { ema: p1 };
            case "sma":
                return { sma: p1 };
            default:
                return { rsi: 14 };
        }
    }, [kindKey, p1, p2]);

    const queueAdd = (id: IndexId) =>
        setPending((prev) => {
            const i = prev.findIndex(
                (e) => e.edit === "remove" && eqIndexId(e.id, id)
            );
            if (i !== -1) {
                const cp = prev.slice();
                cp.splice(i, 1);
                return cp;
            }
            if (prev.some((e) => e.edit === "add" && eqIndexId(e.id, id)))
                return prev;
            return [...prev, { id, edit: "add" }];
        });

    const queueRemove = (id: IndexId) =>
        setPending((prev) => {
            const i = prev.findIndex(
                (e) => e.edit === "add" && eqIndexId(e.id, id)
            );
            if (i !== -1) {
                const cp = prev.slice();
                cp.splice(i, 1);
                return cp;
            }
            if (prev.some((e) => e.edit === "remove" && eqIndexId(e.id, id)))
                return prev;
            return [...prev, { id, edit: "remove" }];
        });

    const discardPending = () => setPending([]);
    const applyPending = async () => {
        if (!market) return;
        if (pending.length === 0) return;
        await sendMarketCmd(market.asset, { editIndicators: pending });
        setPending([]);
    };

    const onSaveLev = async () => {
        if (!market) return;
        const clamped = Math.max(1, Math.min(lev, maxLev));
        await sendMarketCmd(market.asset, { updateLeverage: clamped });
    };
    const onSaveMargin = async () => {
        if (!market) return;
        await sendCommand({
            manualUpdateMargin: [market.asset, Math.max(0, margin)],
        });
    };
    useEffect(() => {
        setPendingStrategy(null);
    }, [market?.asset, market?.params.strategy]);

    if (!market) {
        return (
            <div className="text-app-text mx-auto max-w-7xl px-6 py-8">
                <Link to="/" className={BtnGhost}>
                    <ArrowLeft className="mr-2 h-4 w-4" />
                    Back
                </Link>
                <div className={`${Pane} ${Body} mt-6`}>Market not found.</div>
            </div>
        );
    }

    const marketLev = market.lev ?? 0;
    const marketMargin = market.margin ?? 0;
    const currentStrategy = market.params.strategy;
    const hasPendingStrategy =
        pendingStrategy !== null && pendingStrategy !== currentStrategy;
    const showMinOrderWarning =
        market.margin != null &&
        market.lev != null &&
        market.margin * market.lev < MIN_ORDER_VALUE;
    const handleStrategySelect = (option: Strategy) => {
        setPendingStrategy((prev) => {
            if (option === currentStrategy) return null;
            return prev === option ? null : option;
        });
    };
    const cancelStrategyChange = () => setPendingStrategy(null);
    const applyStrategyChange = async () => {
        if (!pendingStrategy || pendingStrategy === currentStrategy) return;
        try {
            await sendMarketCmd(market.asset, {
                updateStrategy: pendingStrategy,
            });
        } catch (err) {
            console.error("Update strategy failed", err);
            return;
        }
        updateMarketStrategy(market.asset, pendingStrategy);
        setPendingStrategy(null);
    };

    /* ====== UI LAYOUT: rail | center (chart & indicators) | inspector ====== */
    return (
        <div className="bg-surface-tone text-app-text relative z-40 min-h-screen max-w-[3300px] overflow-hidden py-8 pb-80 font-mono">
            <ErrorBanner message={errorMsg} onDismiss={dismissError} />
            <div className="mt-10 mb-1 flex items-center justify-around">
                <div className="relative right-[3vw] flex items-center gap-3">
                    <Link to={`/backtest/${sanitizeAsset(market.asset)}`}>
                        <div className="text-md border-accent-brand-strong/40 text-accent-brand relative right-20 w-fit rounded border px-3 py-1 font-semibold">
                            {"BACKTEST (BETA)"}
                        </div>
                    </Link>

                    <button
                        onClick={() =>
                            handleConfirmToggle(market.asset, market.isPaused)
                        }
                        className={Chip}
                    >
                        {market.isPaused ? "Paused" : "Live"}
                    </button>
                    <h1 className="text-[40px] tracking-widest">
                        {market.asset}
                        <span
                            className={`ml-3 text-[24px] ${leverageColor(marketLev, maxLev)}`}
                        >
                            {marketLev}x
                        </span>
                    </h1>
                </div>
            </div>

            <div className={GridCols}>
                {/* LEFT RAIL — quick stats & knobs */}
                <aside className={Rail}>
                    <div className="space-y-4">
                        <Link
                            to="/"
                            className={`mb-10 w-full text-[20px] ${BtnGhost}`}
                        >
                            <ArrowLeft className="mr-2 h-6 w-6" />
                            Back to Markets
                        </Link>
                        <div className="border-line-subtle bg-ink-20 rounded-lg border p-3">
                            <div className="text-app-text/50 text-[10px] uppercase">
                                Price
                            </div>
                            <div className="mt-1 text-2xl">
                                {market.price == null
                                    ? "—"
                                    : `$${num(market.price, pxDecimals)}`}
                            </div>
                        </div>

                        <div className="border-line-subtle bg-ink-20 rounded-lg border p-3">
                            <div className="flex items-center justify-between">
                                <div className="text-app-text/50 text-[14px] uppercase">
                                    PnL
                                </div>
                                <div className="bg-glow-10 h-1 w-16">
                                    {/* mini bar (cosmetic) */}
                                    <div
                                        className={`h-full ${
                                            (market.pnl ?? 0) >= 0
                                                ? "bg-accent-profit-strong"
                                                : "bg-accent-danger-alt-mid"
                                        }`}
                                        style={{
                                            width: `${Math.min(100, Math.abs(market.pnl ?? 0))}%`,
                                        }}
                                    />
                                </div>
                            </div>
                            <div className="mt-1">
                                <PnlTicker pnl={market.pnl ?? 0} />
                            </div>
                        </div>

                        {/* Leverage stepper */}
                        <div className="border-line-subtle bg-ink-20 rounded-lg border p-3">
                            <div className="text-app-text/50 text-[10px] uppercase">
                                Leverage{" "}
                                <strong className="text-[13px]">
                                    {marketLev}×
                                </strong>
                            </div>
                            <div className="mt-2 flex items-center gap-2">
                                <button
                                    className={BtnGhost}
                                    onClick={() =>
                                        setLev((v) => Math.max(1, v - 1))
                                    }
                                    aria-label="dec lev"
                                >
                                    <Minus className="h-4 w-4" />
                                </button>
                                <input
                                    type="number"
                                    min={1}
                                    max={maxLev}
                                    value={lev}
                                    onChange={(e) =>
                                        setLev(
                                            Math.max(
                                                1,
                                                Math.min(
                                                    maxLev,
                                                    +e.target.value
                                                )
                                            )
                                        )
                                    }
                                    className={`${Input} w-24 text-center`}
                                />
                                <button
                                    className={BtnGhost}
                                    onClick={() =>
                                        setLev((v) => Math.min(maxLev, v + 1))
                                    }
                                    aria-label="inc lev"
                                >
                                    <Plus className="h-4 w-4" />
                                </button>
                            </div>
                            <div className="text-app-text/50 mt-2 text-[11px]">
                                Max: {maxLev}×
                            </div>
                            <button
                                onClick={onSaveLev}
                                className={`${BtnOK} mt-3 w-full`}
                            >
                                Apply Leverage
                            </button>
                        </div>

                        {/* Margin */}
                        <div className="border-line-subtle bg-ink-20 rounded-lg border p-3">
                            {showMinOrderWarning && (
                                <>
                                    <img
                                        src="https://cdn-icons-png.flaticon.com/512/14022/14022507.png"
                                        width="12"
                                        height="12"
                                        alt=""
                                        title=""
                                        className="img-small"
                                    />
                                    <p className="text-accent-brand-strong text-[12px]">
                                        MAX ORDER VALUE is lower than 10$, no
                                        orders can be passed
                                    </p>
                                </>
                            )}
                            <div
                                className="text-app-text/50 cursor-pointer text-[12px] uppercase"
                                onClick={() =>
                                    setMargin(totalMargin + marketMargin)
                                }
                            >
                                Margin (MAX:{" "}
                                {(totalMargin + marketMargin).toFixed(2)}$)
                            </div>
                            <div className="mt-2 items-center gap-2">
                                <div className="flex flex-col py-4">
                                    <input
                                        type="range"
                                        min={0}
                                        max={(
                                            totalMargin + marketMargin
                                        ).toFixed(3)}
                                        step={0.01}
                                        value={margin.toFixed(2)}
                                        onChange={(e) =>
                                            setMargin(+e.target.value)
                                        }
                                        className="bg-surface-range h-2 w-full cursor-pointer"
                                    />
                                    <div className="text-app-text mt-1 flex justify-between text-sm">
                                        <span>{margin.toFixed(2)}$</span>
                                        <span>
                                            {(
                                                (margin /
                                                    (totalMargin +
                                                        marketMargin)) *
                                                100
                                            ).toFixed(1)}
                                            %
                                        </span>
                                    </div>
                                </div>
                                <div className="flex items-end justify-between">
                                    <button
                                        onClick={onSaveMargin}
                                        className={BtnOK}
                                    >
                                        Apply
                                    </button>
                                    <span className="">
                                        Margin: {marketMargin.toFixed(2)} $
                                    </span>
                                </div>
                            </div>
                        </div>

                        {/* Strategy snapshot */}
                        <div className="border-line-subtle bg-ink-20 space-y-2 rounded-lg border p-3 text-[12px]">
                            <h3 className="text-center text-[18px]">
                                Strategy
                            </h3>
                            <div className="text-app-text/50 text-center text-[10px] uppercase">
                                Current
                            </div>
                            <p className="text-center text-[14px] font-semibold">
                                {currentStrategy}
                            </p>
                            <div className="mt-2 grid gap-2">
                                {strategyOptions.map((option) => {
                                    const isCurrent =
                                        option === currentStrategy;
                                    const isPending =
                                        option === pendingStrategy;
                                    return (
                                        <button
                                            key={option}
                                            type="button"
                                            onClick={() =>
                                                handleStrategySelect(option)
                                            }
                                            className={`w-full rounded-md border px-2 py-1 text-[11px] tracking-wide uppercase transition ${
                                                isPending
                                                    ? "border-accent-warning-strong/60 bg-accent-warning-strong/10 text-accent-warning-mid"
                                                    : isCurrent
                                                      ? "border-accent-brand-strong/60 bg-glow-5 text-accent-brand-soft"
                                                      : "border-line-subtle bg-app-surface-3 text-app-text/70 hover:bg-glow-10"
                                            }`}
                                        >
                                            {option}
                                        </button>
                                    );
                                })}
                            </div>
                            {hasPendingStrategy && (
                                <>
                                    <div className="text-accent-warning-mid text-center text-[11px] uppercase">
                                        Pending: {pendingStrategy}
                                    </div>
                                    <div className="border-accent-warning/40 bg-surface-warning rounded-md border px-2 py-1 text-[11px]">
                                        <span className="text-accent-warning-mid font-semibold">
                                            Warning:
                                        </span>{" "}
                                        <span className="text-warning-soft/80">
                                            Changing strategy will close any
                                            open position.
                                        </span>
                                    </div>
                                    <div className="flex gap-2">
                                        <button
                                            type="button"
                                            onClick={cancelStrategyChange}
                                            className={`${BtnGhost} w-full`}
                                        >
                                            Cancel
                                        </button>
                                        <button
                                            type="button"
                                            onClick={applyStrategyChange}
                                            className={`${BtnOK} w-full`}
                                        >
                                            Apply
                                        </button>
                                    </div>
                                </>
                            )}
                        </div>
                    </div>
                </aside>

                {/* CENTER — chart area + active indicators + trades */}
                <main className="space-y-4">
                    {/* Chart placeholder with scanlines */}
                    <section className={`${Pane}`}>
                        <div className={Head}>
                            Chart{" "}
                            <span className="text-accent-danger-muted/50">
                                Note: This is a reference spot price chart,{" "}
                                <a
                                    className="text-accent-highlight font-bold underline"
                                    href={`https://app.hyperliquid.xyz/trade/${market.asset}`}
                                    target="_blank"
                                >
                                    Hyperliquid chart
                                </a>{" "}
                                (PERPS) is likely different
                            </span>
                        </div>
                        <div
                            className={`${Chart} kwant-theme relative h-[60vh]`}
                        >
                            <KwantChart
                                asset={routeAsset}
                                title="KWANT"
                                backgroundColor="rgb(var(--app-surface-2))"
                                gridColor="rgb(var(--app-bg))"
                                secondaryColor="#36c5f0"
                                crosshairColor="rgb(var(--app-text))"
                            />
                        </div>
                    </section>

                    {/* Active indicators list */}
                    <section className={`${Pane} mt-25`}>
                        <div
                            className={`${Head} flex items-center justify-between`}
                        >
                            <span>Active Indicators</span>
                            <span className="text-app-text/40">
                                Count: {market.indicators.length}
                            </span>
                        </div>
                        <div className={`${Body} flex flex-wrap gap-2`}>
                            {market.indicators.map((data, i) => {
                                const { kind, timeframe, value } =
                                    decompose(data);
                                const kindKey = Object.keys(
                                    kind
                                )[0] as IndicatorName;
                                return (
                                    <div className="group border-line-subtle flex flex-col items-center gap-2 rounded-lg border px-2.5 py-1 text-[11px]">
                                        <div
                                            key={`${kindKey}-${fromTimeFrame(timeframe)}-${i}`}
                                            className={`group border-line-subtle flex items-center gap-4 rounded-lg border px-2.5 py-1 text-[13px] ${indicatorColors[kindKey]}`}
                                            title={JSON.stringify(kind)}
                                        >
                                            <span className="font-medium">
                                                {indicatorLabels[kindKey] ||
                                                    kindKey}{" "}
                                                — {fromTimeFrame(timeframe)}
                                            </span>
                                            <button
                                                className="hover:bg-glow-10 rounded p-0.5"
                                                onClick={() =>
                                                    queueRemove([
                                                        kind,
                                                        timeframe,
                                                    ])
                                                }
                                                title="Queue remove"
                                            >
                                                <X className="h-3.5 w-3.5" />
                                            </button>
                                        </div>
                                        <span
                                            className={`text-center text-xl font-bold ${indicatorValueColors[kindKey]}`}
                                        >
                                            {value
                                                ? get_value(value, pxDecimals)
                                                : "N/A"}
                                        </span>
                                    </div>
                                );
                            })}
                        </div>
                    </section>

                    {/* Trades table */}
                    <section className={`${Pane}`}>
                        <div className={Head}>Trades</div>
                        <div className={`${Body} overflow-x-auto`}>
                            {!market.trades || market.trades.length === 0 ? (
                                <div className="text-app-text/60 text-sm">
                                    No trades yet.
                                </div>
                            ) : (
                                <table className="min-w-full text-[12px]">
                                    <thead className="text-app-text/60">
                                        <tr>
                                            <th className="py-2 pr-4 text-left">
                                                Side
                                            </th>
                                            <th className="py-2 pr-4 text-right">
                                                Open
                                            </th>
                                            <th className="py-2 pr-4 text-right">
                                                Close
                                            </th>
                                            <th className="py-2 pr-4 text-right">
                                                PnL
                                            </th>

                                            <th className="py-2 pr-4 text-right">
                                                Size
                                            </th>
                                            <th className="py-2 pr-4 text-right">
                                                Fee
                                            </th>

                                            <th className="py-2 pr-4 text-right">
                                                Funding
                                            </th>

                                            <th className="py-2 text-right">
                                                Open Time - Close Time
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {market.trades.map(
                                            (t: TradeInfo, i: number) => (
                                                <tr
                                                    key={i}
                                                    className="border-line-subtle border-t"
                                                >
                                                    <td
                                                        className={`py-2 pr-4 font-semibold uppercase ${
                                                            t.side == "long"
                                                                ? "text-accent-success-strong"
                                                                : "text-accent-danger"
                                                        }`}
                                                    >
                                                        {t.side}
                                                    </td>
                                                    <td className="py-2 pr-4 text-right">
                                                        {formatPrice(
                                                            t.open.price
                                                        )}
                                                    </td>
                                                    <td className="py-2 pr-4 text-right">
                                                        {formatPrice(
                                                            t.close.price
                                                        )}
                                                    </td>
                                                    <td
                                                        className={`py-2 pr-4 text-right ${
                                                            t.pnl >= 0
                                                                ? "text-accent-profit"
                                                                : "text-accent-danger-alt-soft"
                                                        }`}
                                                    >
                                                        {num(t.pnl, 2)}$
                                                    </td>
                                                    <td className="py-2 pr-4 text-right">
                                                        {num(
                                                            t.size,
                                                            meta?.szDecimals ??
                                                                3
                                                        )}
                                                    </td>

                                                    <td className="py-2 pr-4 text-right">
                                                        {num(t.fees, 2)}$
                                                    </td>
                                                    <td className="py-2 text-right">
                                                        {t.funding}
                                                    </td>

                                                    <td className="py-2 text-right">
                                                        {formatUTC(t.open.time)}{" "}
                                                        -{" "}
                                                        {formatUTC(
                                                            t.close.time
                                                        )}
                                                    </td>
                                                </tr>
                                            )
                                        )}
                                    </tbody>
                                </table>
                            )}
                        </div>
                    </section>
                </main>

                {/* RIGHT — Indicator builder + Pending batch */}
                <aside className="space-y-4">
                    <div className={Pane}>
                        <p className="border-accent-brand-deep/40 border-b py-1 text-center">
                            OPEN POSITION
                        </p>
                        <div className="px-3 py-2">
                            {market.position == null ? (
                                <p className="text-center">No open position</p>
                            ) : (
                                <PositionTable
                                    position={market.position}
                                    price={market.price}
                                    lev={market.lev}
                                    szDecimals={meta?.szDecimals ?? 3}
                                    formatPrice={formatPrice}
                                />
                            )}
                        </div>
                    </div>
                    <section className={Pane}>
                        <div className={Head}>Add Indicator</div>
                        <div className={`${Body} grid grid-cols-2 gap-3`}>
                            <div className="col-span-2">
                                <label className="text-app-text/50 text-[10px] uppercase">
                                    Kind
                                </label>
                                <select
                                    className={Select}
                                    value={kindKey}
                                    onChange={(e) => {
                                        setKindKey(
                                            e.target.value as IndicatorName
                                        );
                                        setP1(14);
                                        setP2(14);
                                    }}
                                >
                                    {kindKeys.map((k) => (
                                        <option
                                            key={k}
                                            value={k}
                                            className="bg-app-surface-3 text-app-text"
                                        >
                                            {indicatorLabels[k] || k}
                                        </option>
                                    ))}
                                </select>
                            </div>

                            <div>
                                <label className="text-app-text/50 text-[10px] uppercase">
                                    {indicatorParamLabels[kindKey]?.[0] ??
                                        "Param"}
                                </label>
                                <input
                                    type="number"
                                    className={Input}
                                    value={p1}
                                    onChange={(e) => setP1(+e.target.value)}
                                />
                            </div>

                            {["emaCross", "smaOnRsi", "adx"].includes(
                                kindKey
                            ) && (
                                <div>
                                    <label className="text-app-text/50 text-[10px] uppercase">
                                        {indicatorParamLabels[kindKey]?.[1] ??
                                            "Param2"}
                                    </label>
                                    <input
                                        type="number"
                                        className={Input}
                                        value={p2}
                                        onChange={(e) => setP2(+e.target.value)}
                                    />
                                </div>
                            )}

                            <div className="col-span-2">
                                <label className="text-app-text/50 text-[10px] uppercase">
                                    Timeframe
                                </label>
                                <select
                                    className={Select}
                                    value={tfSym}
                                    onChange={(e) => setTfSym(e.target.value)}
                                >
                                    {[
                                        "1m",
                                        "3m",
                                        "5m",
                                        "15m",
                                        "30m",
                                        "1h",
                                        "2h",
                                        "4h",
                                        "12h",
                                        "1d",
                                        "3d",
                                        "1w",
                                        "1M",
                                    ].map((s) => (
                                        <option
                                            key={s}
                                            value={s}
                                            className="bg-app-surface-3 text-app-text"
                                        >
                                            {s}
                                        </option>
                                    ))}
                                </select>
                            </div>

                            <div className="col-span-2">
                                <button
                                    onClick={() => {
                                        const id: IndexId = [
                                            buildKind(),
                                            into(tfSym) as TimeFrame,
                                        ];
                                        queueAdd(id);
                                    }}
                                    className={`${BtnGhost} w-full`}
                                >
                                    Queue Add
                                </button>
                            </div>
                        </div>
                    </section>

                    <section className={Pane}>
                        <div
                            className={`${Head} flex items-center justify-between`}
                        >
                            <span>Pending Changes</span>
                            <span className="text-app-text/40">
                                {pending.length}
                            </span>
                        </div>
                        <div className={`${Body}`}>
                            {pending.length === 0 ? (
                                <div className="text-app-text/50 text-[12px]">
                                    No pending edits.
                                </div>
                            ) : (
                                <>
                                    <div className="mb-3 flex flex-wrap gap-2">
                                        {pending.map((e, idx) => {
                                            const [kind, tf] = e.id;
                                            const k = Object.keys(
                                                kind
                                            )[0] as IndicatorName;
                                            return (
                                                <div
                                                    key={idx}
                                                    className={`border-line-subtle flex items-center gap-2 rounded-md border px-2 py-0.5 text-[11px] ${
                                                        e.edit === "add"
                                                            ? "bg-accent-profit-darker/35"
                                                            : "bg-accent-danger-alt-darker/35"
                                                    }`}
                                                >
                                                    <span className="tracking-wide uppercase">
                                                        {e.edit}
                                                    </span>
                                                    <span>
                                                        ·{" "}
                                                        {indicatorLabels[k] ||
                                                            k}{" "}
                                                        — {fromTimeFrame(tf)}
                                                    </span>
                                                    <button
                                                        className="hover:bg-glow-10 rounded p-0.5"
                                                        onClick={() =>
                                                            setPending((prev) =>
                                                                prev.filter(
                                                                    (_, i) =>
                                                                        i !==
                                                                        idx
                                                                )
                                                            )
                                                        }
                                                        title="Remove from batch"
                                                    >
                                                        <X className="h-3.5 w-3.5" />
                                                    </button>
                                                </div>
                                            );
                                        })}
                                    </div>
                                    <div className="flex gap-2">
                                        <button
                                            onClick={discardPending}
                                            className={BtnGhost}
                                        >
                                            Discard
                                        </button>
                                        <button
                                            onClick={applyPending}
                                            className={BtnOK}
                                        >
                                            Apply {pending.length}
                                        </button>
                                    </div>
                                </>
                            )}
                        </div>
                    </section>
                </aside>
            </div>
            <AnimatePresence>
                {marketToToggle && (
                    <motion.div
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        className="fixed inset-0 z-50"
                    >
                        <div
                            className="bg-app-overlay absolute inset-0"
                            onClick={() => setMarketToToggle(null)}
                        />
                        <motion.div
                            initial={{ y: 24, opacity: 0 }}
                            animate={{ y: 0, opacity: 1 }}
                            exit={{ y: 10, opacity: 0 }}
                            className="border-accent-warning/40 bg-surface-warning relative mx-auto mt-28 w-full max-w-md rounded-md border p-6"
                        >
                            <h3 className="text-lg font-semibold">
                                Pause{" "}
                                <span className="text-accent-warning-mid">
                                    {marketToToggle}
                                </span>
                                ?
                            </h3>
                            <p className="text-warning-soft/80 mt-1">
                                This will close any ongoing trade initiated by
                                the Bot.
                            </p>
                            <div className="mt-6 flex justify-end gap-2">
                                <button
                                    className="border-line-weak hover:bg-glow-10 rounded-md border px-4 py-2"
                                    onClick={() => setMarketToToggle(null)}
                                >
                                    Cancel
                                </button>
                                <button
                                    className="text-on-accent bg-accent-warning-strong hover:bg-accent-warning-deep rounded-md px-4 py-2"
                                    onClick={() =>
                                        handleTogglePause(marketToToggle!)
                                    }
                                >
                                    Yes
                                </button>
                            </div>
                        </motion.div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
}
