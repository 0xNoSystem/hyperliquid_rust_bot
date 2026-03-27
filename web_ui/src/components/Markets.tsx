import { useState, useCallback, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
    Plus,
    Power,
    Pause,
    KeyRound,
    RefreshCw,
    CheckCircle,
} from "lucide-react";
import { useNavigate, useLocation } from "react-router-dom";
import MarketCard from "./MarketCard";
import { AddMarket } from "./AddMarket";
import { CachedMarket } from "./CachedMarket";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { ErrorBanner } from "./ErrorBanner";
import LoadingDots from "./Loading";
import type { Strategy } from "../strats";

export default function MarketsPage() {
    const {
        markets,
        universe,
        cachedMarkets,
        totalMargin,
        errorMsg,
        needsApiKey,
        dismissError,
        cacheMarket,
        deleteCachedMarket,
        requestRemoveMarket,
        requestToggleMarket,
        requestCloseAll,
        requestPauseAll,
        requestSyncMargin,
    } = useWebSocketContext();

    const navigate = useNavigate();
    const location = useLocation();
    const [successBanner, setSuccessBanner] = useState<string | null>(null);

    useEffect(() => {
        const state = location.state as { agentApproved?: boolean } | null;
        if (state?.agentApproved) {
            setSuccessBanner(
                "Trading agent approved — your API key is secured."
            );
            window.history.replaceState({}, "");
            const timer = setTimeout(() => setSuccessBanner(null), 5000);
            return () => clearTimeout(timer);
        }
    }, [location.state]);

    const [marketToRemove, setMarketToRemove] = useState<string | null>(null);
    const [marketToToggle, setMarketToToggle] = useState<string | null>(null);
    const [togglingAssets, setTogglingAssets] = useState<Set<string>>(
        new Set()
    );
    const [addInitialAsset, setAddInitialAsset] = useState<
        string | undefined
    >();
    const [showAdd, setShowAdd] = useState(false);
    const [syncingMargin, setSyncingMargin] = useState(false);
    // TODO: fetch strategies from backend via GET /strategies
    const [strategies] = useState<Strategy[]>([]);

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
            setTogglingAssets((prev) => new Set(prev).add(asset));
            requestToggleMarket(asset, false)
                .catch((err) => console.error("Toggle failed", err))
                .finally(() =>
                    setTogglingAssets((prev) => {
                        const next = new Set(prev);
                        next.delete(asset);
                        return next;
                    })
                );
        } else {
            setMarketToToggle(asset);
        }
    };

    const handleTogglePause = (asset: string) => {
        setTogglingAssets((prev) => new Set(prev).add(asset));
        requestToggleMarket(asset, true)
            .catch((err) => console.error("Toggle failed", err))
            .finally(() => {
                setMarketToToggle(null);
                setTogglingAssets((prev) => {
                    const next = new Set(prev);
                    next.delete(asset);
                    return next;
                });
            });
    };

    const handleRemove = (asset: string) => {
        cacheMarket(asset);
        requestRemoveMarket(asset)
            .catch((err) => console.error("Remove failed", err))
            .finally(() => setMarketToRemove(null));
    };

    const openAddModal = useCallback(
        (initialAsset?: string) => {
            requestSyncMargin().catch(console.error);
            setAddInitialAsset(initialAsset);
            setShowAdd(true);
        },
        [requestSyncMargin]
    );

    const handleCloseAdd = () => {
        setShowAdd(false);
        setAddInitialAsset(undefined);
    };

    const handleRestoreCached = (asset: string) => {
        setAddInitialAsset(asset);
        setShowAdd(true);
    };

    const handleSyncMargin = () => {
        setSyncingMargin(true);
        requestSyncMargin()
            .catch(console.error)
            .finally(() => setTimeout(() => setSyncingMargin(false), 600));
    };

    return (
        <div className="bg-app-bg-soft/20 text-app-text relative min-h-screen overflow-hidden pb-100">
            {/* layered background */}
            <div className="max-w-8xl mx-auto mt-20 grid w-[83%] grid-cols-1 gap-8 px-6 py-10 lg:grid-cols-[280px,1fr]">
                {/* Command Dock */}
                <aside className="border-line-subtle bg-surface-pane/30 shadow-panel h-fit rounded-md border p-4">
                    <div className="flex items-baseline justify-between">
                        <div>
                            <div className="text-app-text/50 flex items-center gap-1.5 text-[10px] uppercase">
                                Available Margin
                                <button
                                    onClick={handleSyncMargin}
                                    disabled={syncingMargin}
                                    className="text-app-text/40 hover:text-app-text/80 transition-colors disabled:opacity-50"
                                    title="Refresh margin"
                                >
                                    <RefreshCw
                                        className={`h-3 w-3 ${syncingMargin ? "animate-spin" : ""}`}
                                    />
                                </button>
                            </div>
                            <div className="font-mono text-3xl tracking-tight tabular-nums">
                                {needsApiKey ? (
                                    <button
                                        onClick={() => navigate("/settings")}
                                        className="text-accent-info-link mt-1 flex items-center gap-2 text-sm font-medium hover:underline"
                                    >
                                        <KeyRound className="h-4 w-4" />
                                        Connect API Key
                                    </button>
                                ) : totalMargin ? (
                                    `$${totalMargin.toFixed(2)}`
                                ) : (
                                    <LoadingDots />
                                )}
                            </div>
                        </div>
                        <div className="from-gradient-start via-gradient-mid to-gradient-end h-6 w-1 bg-gradient-to-b" />
                    </div>

                    <div className="mt-4 grid gap-2">
                        {markets.length !== 0 && (
                            <button
                                onClick={() => openAddModal()}
                                className="border-action-add-border bg-action-add-bg text-action-add-text hover:bg-action-add-hover w-full rounded-md border px-3 py-2"
                            >
                                <div className="flex items-center justify-center gap-2">
                                    <Plus className="h-4 w-4" />
                                    <span className="text-sm">Add Market</span>
                                </div>
                            </button>
                        )}
                        <button
                            className="border-action-close-border bg-action-close-bg text-action-close-text hover:bg-action-close-hover w-full rounded-md border px-3 py-2"
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
                            className="border-action-pause-border bg-action-pause-bg text-action-pause-text hover:bg-action-pause-hover w-full rounded-md border px-3 py-2"
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

                    <div className="border-line-subtle text-app-text/60 mt-6 grid gap-2 border-t pt-4 text-[12px]">
                        <div className="p-2 text-right text-[25px] font-bold">
                            PnL : {<SessionPnlDisplay />}
                        </div>
                        <p className="text-app-text/70 font-semibold">
                            Recent Markets
                        </p>

                        <div className="border-line-subtle h-43 overflow-y-auto rounded-md border bg-transparent p-3">
                            {cachedMarkets.length === 0 ? (
                                <p className="text-app-text/40 italic">
                                    No cached markets.
                                </p>
                            ) : (
                                cachedMarkets.map((asset) => (
                                    <CachedMarket
                                        key={asset}
                                        asset={asset}
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
                        <div className="border-line-subtle bg-surface-pane grid place-items-center rounded-md border p-12 text-center">
                            {needsApiKey ? (
                                <div>
                                    <KeyRound className="text-app-text/30 mx-auto h-12 w-12" />
                                    <h2 className="mt-4 text-2xl font-semibold">
                                        Connect your API key
                                    </h2>
                                    <p className="text-app-text/60 mt-1">
                                        Add your Hyperliquid API key to start
                                        trading.
                                    </p>
                                    <button
                                        onClick={() => navigate("/settings")}
                                        className="border-action-add-border bg-action-add-bg text-action-add-text hover:bg-action-add-hover mt-5 inline-flex items-center gap-2 rounded-md border px-4 py-2"
                                    >
                                        <KeyRound className="h-4 w-4" /> Go to
                                        Settings
                                    </button>
                                </div>
                            ) : (
                                <div>
                                    <h2 className="text-2xl font-semibold">
                                        No markets configured
                                    </h2>
                                    <p className="text-app-text/60 mt-1">
                                        Add a market to begin streaming quotes
                                        and executing strategies.
                                    </p>
                                    <button
                                        onClick={() => openAddModal()}
                                        className="border-action-add-border bg-action-add-bg text-action-add-text hover:bg-action-add-hover mt-5 inline-flex items-center gap-2 rounded-md border px-4 py-2"
                                    >
                                        <Plus className="h-4 w-4" /> Add Market
                                    </button>
                                </div>
                            )}
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
                                        isToggling={togglingAssets.has(m.asset)}
                                    />
                                </motion.div>
                            ))}
                        </div>
                    )}
                </main>
            </div>
            {/* Error toast */}
            <ErrorBanner message={errorMsg} onDismiss={dismissError} />
            {/* Success banner */}
            <AnimatePresence>
                {successBanner && (
                    <motion.div
                        initial={{ y: -16, opacity: 0 }}
                        animate={{ y: 0, opacity: 1 }}
                        exit={{ y: -16, opacity: 0 }}
                        className="fixed top-6 left-1/2 z-50 -translate-x-1/2"
                    >
                        <div className="border-accent-success-strong/40 bg-surface-success text-success-faint flex items-center gap-2 rounded-md border px-4 py-2 shadow">
                            <CheckCircle className="h-4 w-4" />
                            <span className="text-sm">{successBanner}</span>
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
            {/* Add Market modal */}
            {showAdd && (
                <div className="fixed inset-0 z-50">
                    {/* Overlay */}
                    <div
                        className="bg-app-overlay absolute inset-0"
                        onClick={handleCloseAdd}
                    />

                    {/* Centered Modal */}
                    <div className="absolute inset-0 flex items-center justify-center p-4">
                        <div className="bg-surface-modal rounded-xl shadow-xl">
                            <AddMarket
                                onClose={handleCloseAdd}
                                totalMargin={totalMargin}
                                assets={universe}
                                strategies={strategies}
                                initialAsset={addInitialAsset}
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
                            className="bg-app-overlay absolute inset-0"
                            onClick={() => setMarketToRemove(null)}
                        />
                        <motion.div
                            initial={{ y: 24, opacity: 0 }}
                            animate={{ y: 0, opacity: 1 }}
                            exit={{ y: 10, opacity: 0 }}
                            className="border-accent-danger/40 bg-surface-danger-soft relative mx-auto mt-28 w-full max-w-md rounded-md border p-6"
                        >
                            <h3 className="text-lg font-semibold">
                                Remove{" "}
                                <span className="text-accent-danger-muted">
                                    {marketToRemove}
                                </span>
                                ?
                            </h3>
                            <p className="text-danger-soft/80 mt-1">
                                This will close any ongoing trade initiated by
                                the Bot.
                            </p>
                            <div className="mt-6 flex justify-end gap-2">
                                <button
                                    className="border-line-weak hover:bg-glow-10 rounded-md border px-4 py-2"
                                    onClick={() => setMarketToRemove(null)}
                                >
                                    Cancel
                                </button>
                                <button
                                    className="text-on-accent bg-accent-danger-strong hover:bg-accent-danger-deep rounded-md px-4 py-2"
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
            <style>{`@keyframes scan{0%{transform:translateX(0)}100%{transform:translateX(-25%)}}`}</style>
        </div>
    );
}
