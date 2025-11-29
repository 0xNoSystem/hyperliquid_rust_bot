import React, { useState } from "react";
import { Link } from "react-router-dom";
import { motion, AnimatePresence } from "framer-motion";
import { Plus, Power, Pause, X, AlertCircle } from "lucide-react";
import MarketCard from "./MarketCard";
import { AddMarket } from "./AddMarket";
import { CachedMarket } from "./CachedMarket";
import { useWebSocketContext } from "../context/WebSocketContext";
import { BackgroundFX } from "../components/BackgroundFX";
import LoadingDots from "./Loading";

export default function MarketsPage() {
    const {
        markets,
        universe,
        cachedMarkets,
        totalMargin,
        errorMsg,
        sendCommand,
        dismissError,
        cacheMarket,
        deleteCachedMarket,
        requestRemoveMarket,
        requestToggleMarket,
        requestCloseAll,
        requestPauseAll,
    } = useWebSocketContext();

    const [marketToRemove, setMarketToRemove] = useState<string | null>(null);
    const [marketToToggle, setMarketToToggle] = useState<string | null>(null);
    const [showAdd, setShowAdd] = useState(false);

    const sessionPnl = markets.reduce((sum, market) => {
        return sum + (market.pnl ?? 0);
    }, 0);

    const SessionPnlDisplay = () => (
        <span
            className={`font-mono tabular-nums ${sessionPnl >= 0 ? "text-green-400" : "text-red-400"}`}
        >
            {sessionPnl >= 0 ? "+" : ""}
            {sessionPnl.toFixed(2)}$
        </span>
    );

    const handleConfirmToggle = (asset: string, isPaused: boolean) => {
        if (isPaused) {
            requestToggleMarket(asset).catch((err) =>
                console.error("Toggle failed", err)
            );
        } else {
            setMarketToToggle(asset);
        }
    };

    const handleTogglePause = (asset: string) => {
        requestToggleMarket(asset)
            .catch((err) => console.error("Toggle failed", err))
            .finally(() => setMarketToToggle(null));
    };

    const handleRemove = (asset: string) => {
        const market = markets.find((m) => m.asset === asset);
        if (market) {
            cacheMarket(market);
        }
        requestRemoveMarket(asset)
            .catch((err) => console.error("Remove failed", err))
            .finally(() => setMarketToRemove(null));
    };

    const handleRestoreCached = async (asset: string) => {
        const info = cachedMarkets.find((cm) => cm.asset === asset);
        if (!info) return;
        try {
            const res = await sendCommand({ addMarket: info });
            if (res.ok) {
                deleteCachedMarket(asset);
            }
        } catch (error) {
            console.error("Failed to restore market", error);
        }
    };

    return (
        <div className="relative min-h-screen overflow-hidden bg-[#07090B] pb-100 text-white">
            {/* layered background */}
            <BackgroundFX intensity={1} />
            <div className="mx-auto mt-20 grid max-w-7xl grid-cols-1 gap-8 px-6 py-10 lg:grid-cols-[280px,1fr]">
                {/* Command Dock */}
                <aside className="h-fit rounded-md border border-white/10 bg-[#0B0E12]/80 p-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.05)]">
                    <div className="flex items-baseline justify-between">
                        <div>
                            <div className="text-[10px] text-white/50 uppercase">
                                Available Margin
                            </div>
                            <div className="font-mono text-3xl tracking-tight tabular-nums">
                                {totalMargin ? (
                                    `$${totalMargin.toFixed(2)}`
                                ) : (
                                    <LoadingDots />
                                )}
                            </div>
                        </div>
                        <div className="h-6 w-1 bg-gradient-to-b from-cyan-400 via-fuchsia-400 to-emerald-400" />
                    </div>

                    <div className="mt-4 grid gap-2">
                        {markets.length !== 0 && (
                            <button
                                onClick={() => setShowAdd(true)}
                                className="w-full rounded-md border border-cyan-400/40 bg-cyan-500/10 px-3 py-2 text-cyan-200 hover:bg-cyan-500/20"
                            >
                                <div className="flex items-center justify-center gap-2">
                                    <Plus className="h-4 w-4" />
                                    <span className="text-sm">Add Market</span>
                                </div>
                            </button>
                        )}
                        <button
                            className="w-full rounded-md border border-red-500/40 bg-red-600/15 px-3 py-2 text-red-200 hover:bg-red-600/25"
                            onClick={() =>
                                requestCloseAll().catch((err) =>
                                    console.error("Close all failed", err)
                                )
                            }
                        >
                            <div className="flex items-center justify-center gap-2">
                                <Power className="h-4 w-4" />
                                <span className="text-sm">Close All</span>
                            </div>
                        </button>
                        <button
                            className="w-full rounded-md border border-amber-500/40 bg-amber-500/15 px-3 py-2 text-amber-200 hover:bg-amber-500/25"
                            onClick={() =>
                                requestPauseAll().catch((err) =>
                                    console.error("Pause all failed", err)
                                )
                            }
                        >
                            <div className="flex items-center justify-center gap-2">
                                <Pause className="h-4 w-4" />
                                <span className="text-sm">Pause All</span>
                            </div>
                        </button>
                    </div>

                    <div className="mt-6 grid gap-2 border-t border-white/10 pt-4 text-[12px] text-white/60">
                        <div className="p-2 text-right text-[25px] font-bold">
                            PnL : {<SessionPnlDisplay />}
                        </div>
                        <p className="font-semibold text-white/70">Console</p>

                        <div className="h-43 overflow-y-auto rounded-md border border-white/10 bg-[#0F1115] p-3">
                            {cachedMarkets.length === 0 ? (
                                <p className="text-white/40 italic">
                                    No cached markets.
                                </p>
                            ) : (
                                cachedMarkets.map((m) => (
                                    <CachedMarket
                                        key={m.asset}
                                        market={m}
                                        onAdd={handleRestoreCached}
                                        onRemove={deleteCachedMarket}
                                    />
                                ))
                            )}
                        </div>
                    </div>
                </aside>

                {/* Markets Grid */}
                <main>
                    {markets.length === 0 && (
                        <div className="grid place-items-center rounded-md border border-white/10 bg-[#0B0E12]/80 p-12 text-center">
                            <div>
                                <h2 className="text-2xl font-semibold">
                                    No markets configured
                                </h2>
                                <p className="mt-1 text-white/60">
                                    Add a market to begin streaming quotes and
                                    executing strategies.
                                </p>
                                <button
                                    onClick={() => setShowAdd(true)}
                                    className="mt-5 inline-flex items-center gap-2 rounded-md border border-cyan-400/40 bg-cyan-500/10 px-4 py-2 text-cyan-200 hover:bg-cyan-500/20"
                                >
                                    <Plus className="h-4 w-4" /> Add Market
                                </button>
                            </div>
                        </div>
                    )}

                    {markets.length > 0 && (
                        <div className="grid grid-cols-1 gap-7 sm:grid-cols-2 xl:grid-cols-3">
                            {markets.map((m) => (
                                <motion.div
                                    key={m.asset}
                                    initial={{ opacity: 0, y: 10 }}
                                    animate={{ opacity: 1, y: 0 }}
                                >
                                    <MarketCard
                                        market={m}
                                        onTogglePause={() =>
                                            handleConfirmToggle(
                                                m.asset,
                                                m.isPaused
                                            )
                                        }
                                        onRemove={() =>
                                            setMarketToRemove(m.asset)
                                        }
                                    />
                                </motion.div>
                            ))}
                        </div>
                    )}
                </main>
            </div>
            {/* Error toast */}
            <AnimatePresence>
                {errorMsg && (
                    <motion.div
                        initial={{ y: -16, opacity: 0 }}
                        animate={{ y: 0, opacity: 1 }}
                        exit={{ y: -16, opacity: 0 }}
                        className="fixed top-6 left-1/2 z-50 -translate-x-1/2"
                    >
                        <div className="flex items-center gap-2 rounded-md border border-red-500/40 bg-[#2A1010] px-3 py-2 text-red-100 shadow">
                            <AlertCircle className="h-4 w-4" />
                            <span className="text-sm">{errorMsg}</span>
                            <button
                                onClick={dismissError}
                                className="ml-2 rounded-md px-2 py-1 hover:bg-white/10"
                            >
                                <X className="h-4 w-4" />
                            </button>
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
            {/* Add Market modal */}
            {showAdd && (
                <div className="fixed inset-0 z-50">
                    {/* Overlay */}
                    <div
                        className="absolute inset-0 bg-black/70"
                        onClick={() => setShowAdd(false)}
                    />

                    {/* Centered Modal */}
                    <div className="absolute inset-0 flex items-center justify-center p-4">
                        <div className="rounded-xl bg-neutral-900 shadow-xl">
                            <AddMarket
                                onClose={() => setShowAdd(false)}
                                totalMargin={totalMargin}
                                assets={universe}
                            />
                        </div>
                    </div>
                </div>
            )}{" "}
            {/* Confirm remove */}
            <AnimatePresence>
                {marketToRemove && (
                    <motion.div
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        className="fixed inset-0 z-50"
                    >
                        <div
                            className="absolute inset-0 bg-black/70"
                            onClick={() => setMarketToRemove(null)}
                        />
                        <motion.div
                            initial={{ y: 24, opacity: 0 }}
                            animate={{ y: 0, opacity: 1 }}
                            exit={{ y: 10, opacity: 0 }}
                            className="relative mx-auto mt-28 w-full max-w-md rounded-md border border-red-500/40 bg-[#1A0F12] p-6"
                        >
                            <h3 className="text-lg font-semibold">
                                Remove{" "}
                                <span className="text-red-300">
                                    {marketToRemove}
                                </span>
                                ?
                            </h3>
                            <p className="mt-1 text-red-200/80">
                                This will close any ongoing trade initiated by
                                the Bot.
                            </p>
                            <div className="mt-6 flex justify-end gap-2">
                                <button
                                    className="rounded-md border border-white/20 px-4 py-2 hover:bg-white/10"
                                    onClick={() => setMarketToRemove(null)}
                                >
                                    Cancel
                                </button>
                                <button
                                    className="rounded-md bg-red-600 px-4 py-2 text-white hover:bg-red-700"
                                    onClick={() =>
                                        handleRemove(marketToRemove!)
                                    }
                                >
                                    Yes
                                </button>
                            </div>
                        </motion.div>
                    </motion.div>
                )}
            </AnimatePresence>
            {/* Confirm pause */}
            <AnimatePresence>
                {marketToToggle && (
                    <motion.div
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        className="fixed inset-0 z-50"
                    >
                        <div
                            className="absolute inset-0 bg-black/70"
                            onClick={() => setMarketToToggle(null)}
                        />
                        <motion.div
                            initial={{ y: 24, opacity: 0 }}
                            animate={{ y: 0, opacity: 1 }}
                            exit={{ y: 10, opacity: 0 }}
                            className="relative mx-auto mt-28 w-full max-w-md rounded-md border border-amber-500/40 bg-[#1A140A] p-6"
                        >
                            <h3 className="text-lg font-semibold">
                                Pause{" "}
                                <span className="text-amber-300">
                                    {marketToToggle}
                                </span>
                                ?
                            </h3>
                            <p className="mt-1 text-amber-200/80">
                                This will close any ongoing trade initiated by
                                the Bot.
                            </p>
                            <div className="mt-6 flex justify-end gap-2">
                                <button
                                    className="rounded-md border border-white/20 px-4 py-2 hover:bg-white/10"
                                    onClick={() => setMarketToToggle(null)}
                                >
                                    Cancel
                                </button>
                                <button
                                    className="rounded-md bg-amber-600 px-4 py-2 text-white hover:bg-amber-700"
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
            <style>{`@keyframes scan{0%{transform:translateX(0)}100%{transform:translateX(-25%)}}`}</style>
        </div>
    );
}
