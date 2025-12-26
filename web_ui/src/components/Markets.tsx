import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Plus, Power, Pause } from "lucide-react";
import MarketCard from "./MarketCard";
import { AddMarket } from "./AddMarket";
import { CachedMarket } from "./CachedMarket";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { ErrorBanner } from "./ErrorBanner";
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
            className={`font-mono tabular-nums ${
                sessionPnl >= 0
                    ? "text-accent-success"
                    : "text-accent-danger-soft"
            }`}
        >
            {sessionPnl >= 0 ? "+" : ""}
            {sessionPnl.toFixed(2)}$
        </span>
    );

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
        <div className="relative min-h-screen overflow-hidden bg-app-bg-soft pb-100 text-app-text">
            {/* layered background */}
            <div className="max-w-8xl mx-auto mt-20 grid w-[83%] grid-cols-1 gap-8 px-6 py-10 lg:grid-cols-[280px,1fr]">
                {/* Command Dock */}
                <aside className="h-fit rounded-md border border-line-subtle bg-surface-pane p-4 shadow-panel">
                    <div className="flex items-baseline justify-between">
                        <div>
                            <div className="text-[10px] text-app-text/50 uppercase">
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
                        <div className="h-6 w-1 bg-gradient-to-b from-gradient-start via-gradient-mid to-gradient-end" />
                    </div>

                    <div className="mt-4 grid gap-2">
                        {markets.length !== 0 && (
                            <button
                                onClick={() => setShowAdd(true)}
                                className="w-full rounded-md border border-action-add-border bg-action-add-bg px-3 py-2 text-action-add-text hover:bg-action-add-hover"
                            >
                                <div className="flex items-center justify-center gap-2">
                                    <Plus className="h-4 w-4" />
                                    <span className="text-sm">Add Market</span>
                                </div>
                            </button>
                        )}
                        <button
                            className="w-full rounded-md border border-action-close-border bg-action-close-bg px-3 py-2 text-action-close-text hover:bg-action-close-hover"
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
                            className="w-full rounded-md border border-action-pause-border bg-action-pause-bg px-3 py-2 text-action-pause-text hover:bg-action-pause-hover"
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

                    <div className="mt-6 grid gap-2 border-t border-line-subtle pt-4 text-[12px] text-app-text/60">
                        <div className="p-2 text-right text-[25px] font-bold">
                            PnL : {<SessionPnlDisplay />}
                        </div>
                        <p className="font-semibold text-app-text/70">
                            Recent Markets
                        </p>

                        <div className="h-43 overflow-y-auto rounded-md border border-line-subtle bg-transparent p-3">
                            {cachedMarkets.length === 0 ? (
                                <p className="text-app-text/40 italic">
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
                        <div className="grid place-items-center rounded-md border border-line-subtle bg-surface-pane p-12 text-center">
                            <div>
                                <h2 className="text-2xl font-semibold">
                                    No markets configured
                                </h2>
                                <p className="mt-1 text-app-text/60">
                                    Add a market to begin streaming quotes and
                                    executing strategies.
                                </p>
                                <button
                                    onClick={() => setShowAdd(true)}
                                    className="mt-5 inline-flex items-center gap-2 rounded-md border border-action-add-border bg-action-add-bg px-4 py-2 text-action-add-text hover:bg-action-add-hover"
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
                                        assetMeta={universe.find(
                                            (u) => u.name === m.asset
                                        )}
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
            <ErrorBanner message={errorMsg} onDismiss={dismissError} />
            {/* Add Market modal */}
            {showAdd && (
                <div className="fixed inset-0 z-50">
                    {/* Overlay */}
                    <div
                        className="absolute inset-0 bg-app-overlay"
                        onClick={() => setShowAdd(false)}
                    />

                    {/* Centered Modal */}
                    <div className="absolute inset-0 flex items-center justify-center p-4">
                        <div className="rounded-xl bg-surface-modal shadow-xl">
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
                            className="absolute inset-0 bg-app-overlay"
                            onClick={() => setMarketToRemove(null)}
                        />
                        <motion.div
                            initial={{ y: 24, opacity: 0 }}
                            animate={{ y: 0, opacity: 1 }}
                            exit={{ y: 10, opacity: 0 }}
                            className="relative mx-auto mt-28 w-full max-w-md rounded-md border border-accent-danger/40 bg-surface-danger-soft p-6"
                        >
                            <h3 className="text-lg font-semibold">
                                Remove{" "}
                                <span className="text-accent-danger-muted">
                                    {marketToRemove}
                                </span>
                                ?
                            </h3>
                            <p className="mt-1 text-danger-soft/80">
                                This will close any ongoing trade initiated by
                                the Bot.
                            </p>
                            <div className="mt-6 flex justify-end gap-2">
                                <button
                                    className="rounded-md border border-line-weak px-4 py-2 hover:bg-glow-10"
                                    onClick={() => setMarketToRemove(null)}
                                >
                                    Cancel
                                </button>
                                <button
                                    className="text-on-accent rounded-md bg-accent-danger-strong px-4 py-2 hover:bg-accent-danger-deep"
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
                            className="absolute inset-0 bg-app-overlay"
                            onClick={() => setMarketToToggle(null)}
                        />
                        <motion.div
                            initial={{ y: 24, opacity: 0 }}
                            animate={{ y: 0, opacity: 1 }}
                            exit={{ y: 10, opacity: 0 }}
                            className="relative mx-auto mt-28 w-full max-w-md rounded-md border border-accent-warning/40 bg-surface-warning p-6"
                        >
                            <h3 className="text-lg font-semibold">
                                Pause{" "}
                                <span className="text-accent-warning-mid">
                                    {marketToToggle}
                                </span>
                                ?
                            </h3>
                            <p className="mt-1 text-warning-soft/80">
                                This will close any ongoing trade initiated by
                                the Bot.
                            </p>
                            <div className="mt-6 flex justify-end gap-2">
                                <button
                                    className="rounded-md border border-line-weak px-4 py-2 hover:bg-glow-10"
                                    onClick={() => setMarketToToggle(null)}
                                >
                                    Cancel
                                </button>
                                <button
                                    className="text-on-accent rounded-md bg-accent-warning-strong px-4 py-2 hover:bg-accent-warning-deep"
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
