import { useState, useEffect, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { num } from "../types";
import type { BacktestRunEntry } from "../types";
import { useAuth } from "../context/AuthContextStore";
import {
    fetchBacktestHistory,
    deleteBacktestRun,
} from "../api/backtest";

const PAGE_SIZE = 50;

interface BacktestHistoryProps {
    asset?: string;
    offset?: number;
}

export default function BacktestHistory({
    asset,
    offset: initialOffset,
}: BacktestHistoryProps) {
    const { token } = useAuth();
    const nav = useNavigate();

    const [runs, setRuns] = useState<BacktestRunEntry[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [offset, setOffset] = useState(initialOffset ?? 0);
    const [hasMore, setHasMore] = useState(false);

    const load = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            const data = await fetchBacktestHistory(token, {
                asset,
                limit: PAGE_SIZE + 1,
                offset,
            });
            if (data.length > PAGE_SIZE) {
                setHasMore(true);
                setRuns(data.slice(0, PAGE_SIZE));
            } else {
                setHasMore(false);
                setRuns(data);
            }
        } catch (e: unknown) {
            setError(e instanceof Error ? e.message : "Failed to load history");
        } finally {
            setLoading(false);
        }
    }, [token, asset, offset]);

    useEffect(() => {
        load();
    }, [load]);

    const handleDelete = async (id: string) => {
        try {
            await deleteBacktestRun(token, id);
            setRuns((prev) => prev.filter((r) => r.id !== id));
        } catch {
            // silent — row stays visible
        }
    };

    const fmtDate = (ts: number) =>
        new Date(ts).toISOString().slice(0, 16).replace("T", " ");

    return (
        <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
                <h2 className="text-app-text text-sm font-semibold">
                    Backtest History
                    {asset && (
                        <span className="text-app-text/50 ml-1 font-normal">
                            — {asset}
                        </span>
                    )}
                </h2>
                <button
                    className="border-line-subtle text-app-text/70 hover:text-app-text cursor-pointer rounded border px-2 py-1 text-xs transition-colors"
                    onClick={load}
                >
                    Refresh
                </button>
            </div>

            {loading && (
                <p className="text-app-text/50 text-xs">Loading...</p>
            )}
            {error && (
                <p className="text-accent-danger-soft text-xs">{error}</p>
            )}

            {!loading && runs.length === 0 && (
                <p className="text-app-text/40 py-6 text-center text-xs">
                    No backtest runs found.
                </p>
            )}

            {runs.length > 0 && (
                <div className="overflow-auto">
                    <table className="w-full min-w-[900px] text-left text-xs">
                        <thead className="text-app-text/60 border-line-subtle border-b uppercase">
                            <tr>
                                <th className="py-2 pr-3">Asset</th>
                                <th className="py-2 pr-3">Strategy</th>
                                <th className="py-2 pr-3">Exchange</th>
                                <th className="py-2 pr-3 text-right">
                                    Net PnL
                                </th>
                                <th className="py-2 pr-3 text-right">
                                    Return %
                                </th>
                                <th className="py-2 pr-3 text-right">
                                    Max DD %
                                </th>
                                <th className="py-2 pr-3 text-right">
                                    Trades
                                </th>
                                <th className="py-2 pr-3 text-right">
                                    Win Rate
                                </th>
                                <th className="py-2 pr-3">Window</th>
                                <th className="py-2 pr-3">Ran At</th>
                                <th className="py-2"></th>
                            </tr>
                        </thead>
                        <tbody>
                            {runs.map((run) => (
                                <tr
                                    key={run.id}
                                    className="border-line-subtle hover:bg-ink-70 border-b last:border-b-0 transition-colors"
                                >
                                    <td className="text-app-text py-2 pr-3 font-medium">
                                        {run.asset}
                                    </td>
                                    <td className="text-app-text/80 py-2 pr-3">
                                        {run.strategyName || run.strategyId}
                                    </td>
                                    <td className="text-app-text/70 py-2 pr-3 uppercase">
                                        {run.exchange} / {run.market}
                                    </td>
                                    <td
                                        className={`py-2 pr-3 text-right ${
                                            run.netPnl >= 0
                                                ? "text-accent-success"
                                                : "text-accent-danger-soft"
                                        }`}
                                    >
                                        {run.netPnl >= 0 ? "+" : ""}
                                        {num(run.netPnl, 2)}
                                    </td>
                                    <td className="py-2 pr-3 text-right">
                                        {num(run.returnPct, 2)}%
                                    </td>
                                    <td className="py-2 pr-3 text-right">
                                        {num(run.maxDrawdownPct, 2)}%
                                    </td>
                                    <td className="py-2 pr-3 text-right">
                                        {run.totalTrades}
                                    </td>
                                    <td className="py-2 pr-3 text-right">
                                        {num(run.winRatePct, 1)}%
                                    </td>
                                    <td className="text-app-text/60 py-2 pr-3">
                                        {fmtDate(run.startTime)} →{" "}
                                        {fmtDate(run.endTime)}
                                    </td>
                                    <td className="text-app-text/50 py-2 pr-3">
                                        {fmtDate(run.finishedAt)}
                                    </td>
                                    <td className="py-2 text-right">
                                        <div className="flex items-center justify-end gap-1">
                                            <button
                                                className="border-line-subtle text-app-text/60 hover:text-app-text cursor-pointer rounded border px-2 py-0.5 text-xs transition-colors"
                                                onClick={() =>
                                                    nav(
                                                        `/backtest/run/${run.id}`
                                                    )
                                                }
                                            >
                                                Detail
                                            </button>
                                            <button
                                                className="border-line-subtle text-accent-danger-soft/60 hover:text-accent-danger cursor-pointer rounded border px-2 py-0.5 text-xs transition-colors"
                                                onClick={() =>
                                                    handleDelete(run.id)
                                                }
                                            >
                                                Delete
                                            </button>
                                        </div>
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                </div>
            )}

            {(offset > 0 || hasMore) && (
                <div className="flex items-center justify-between pt-2">
                    <button
                        className="border-line-subtle text-app-text/70 hover:text-app-text cursor-pointer rounded border px-3 py-1 text-xs transition-colors disabled:cursor-default disabled:opacity-30"
                        disabled={offset === 0}
                        onClick={() =>
                            setOffset((o) => Math.max(0, o - PAGE_SIZE))
                        }
                    >
                        Previous
                    </button>
                    <span className="text-app-text/40 text-xs">
                        {offset + 1} – {offset + runs.length}
                    </span>
                    <button
                        className="border-line-subtle text-app-text/70 hover:text-app-text cursor-pointer rounded border px-3 py-1 text-xs transition-colors disabled:cursor-default disabled:opacity-30"
                        disabled={!hasMore}
                        onClick={() => setOffset((o) => o + PAGE_SIZE)}
                    >
                        Next
                    </button>
                </div>
            )}
        </div>
    );
}
