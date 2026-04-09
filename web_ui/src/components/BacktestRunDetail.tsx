import { useState, useEffect, useMemo, useCallback } from "react";
import { useParams, useNavigate, useLocation } from "react-router-dom";
import { useAuth } from "../context/AuthContextStore";
import { fetchBacktestResult } from "../api/backtest";
import type {
    BacktestResultDetail,
    BacktestResult as BacktestResultType,
} from "../types";
import { num, formatPrice } from "../types";
import { formatUTC } from "../chart/utils";
import LineChart from "../chart/LineChart";
import LineChartsContainer from "../chart/LineChartsContainer";
import type { LineSeries } from "../chart/LineChart";

function formatUtcMinute(ts: number): string {
    return new Date(ts).toISOString().slice(0, 16).replace("T", " ") + " UTC";
}

/** Convert a live BacktestResult into the shape BacktestResultDetail uses */
function resultToDetail(r: BacktestResultType): BacktestResultDetail {
    return {
        id: r.runId,
        runId: r.runId,
        initialEquity: r.summary.initialEquity,
        finalEquity: r.summary.finalEquity,
        grossProfit: r.summary.grossProfit,
        grossLoss: r.summary.grossLoss,
        avgWin: r.summary.avgWin,
        avgLoss: r.summary.avgLoss,
        expectancy: r.summary.expectancy,
        wins: r.summary.wins,
        losses: r.summary.losses,
        candlesLoaded: r.candlesLoaded,
        candlesProcessed: r.candlesProcessed,
        maxDrawdownAbs: r.summary.maxDrawdownAbs,
        trades: r.trades,
        equityCurve: r.equityCurve,
        snapshots: r.snapshots,
    };
}

export default function BacktestRunDetail() {
    const { runId } = useParams<{ runId: string }>();
    const { token } = useAuth();
    const nav = useNavigate();
    const location = useLocation();

    // If navigated from BacktestResult with state, use it directly
    const passedResult = (location.state as { result?: BacktestResultType })
        ?.result;

    const [detail, setDetail] = useState<BacktestResultDetail | null>(
        passedResult ? resultToDetail(passedResult) : null
    );
    const [loading, setLoading] = useState(!passedResult);
    const [error, setError] = useState<string | null>(null);
    const [chartStart, setChartStart] = useState(0);
    const [chartEnd, setChartEnd] = useState(0);

    useEffect(() => {
        // Skip fetch if we already have data from router state
        if (passedResult || !runId) return;
        let cancelled = false;
        setLoading(true);
        setError(null);

        fetchBacktestResult(token, runId)
            .then((data) => {
                if (!cancelled) setDetail(data);
            })
            .catch((e) => {
                if (!cancelled)
                    setError(
                        e instanceof Error ? e.message : "Failed to load result"
                    );
            })
            .finally(() => {
                if (!cancelled) setLoading(false);
            });

        return () => {
            cancelled = true;
        };
    }, [token, runId, passedResult]);

    // Initialize chart time range when detail loads
    useEffect(() => {
        if (!detail || detail.equityCurve.length === 0) return;
        const first = detail.equityCurve[0].ts;
        const last = detail.equityCurve[detail.equityCurve.length - 1].ts;
        const padding = (last - first) * 0.02;
        setChartStart(first - padding);
        setChartEnd(last + padding);
    }, [detail]);

    const handleTimeRangeChange = useCallback((start: number, end: number) => {
        setChartStart(start);
        setChartEnd(end);
    }, []);

    const equitySeries = useMemo<LineSeries[]>(() => {
        if (!detail) return [];
        const curve = detail.equityCurve;
        if (curve.length === 0) return [];
        return [
            {
                label: "Equity",
                color: "#cf7b15",
                points: curve.map((p) => ({ ts: p.ts, value: p.equity })),
            },
            {
                label: "Balance",
                color: "#6b7280",
                lineWidth: 1,
                points: curve.map((p) => ({ ts: p.ts, value: p.balance })),
            },
        ];
    }, [detail]);

    const upnlSeries = useMemo<LineSeries[]>(() => {
        if (!detail) return [];
        const curve = detail.equityCurve;
        if (curve.length === 0) return [];
        return [
            {
                label: "uPnL",
                color: "#22c55e",
                points: curve.map((p) => ({ ts: p.ts, value: p.upnl })),
            },
        ];
    }, [detail]);

    // Cumulative realized PnL from trades
    const cumulativePnlSeries = useMemo<LineSeries[]>(() => {
        if (!detail || detail.trades.length === 0) return [];
        let cumPnl = 0;
        const points = detail.trades.map((t) => {
            cumPnl += t.pnl;
            return { ts: t.close.time, value: cumPnl };
        });
        return [
            {
                label: "Cumulative PnL",
                color: "#a855f7",
                points,
            },
        ];
    }, [detail]);

    if (loading) {
        return (
            <div className="flex flex-1 items-center justify-center">
                <p className="text-app-text/50 text-sm">
                    Loading run detail...
                </p>
            </div>
        );
    }

    if (error || !detail) {
        return (
            <div className="flex flex-1 flex-col items-center justify-center gap-3">
                <p className="text-accent-danger-soft text-sm">
                    {error ?? "Result not found."}
                </p>
                <button
                    className="border-line-subtle text-app-text/70 hover:text-app-text cursor-pointer rounded border px-3 py-1 text-xs transition-colors"
                    onClick={() => nav(-1)}
                >
                    Go back
                </button>
            </div>
        );
    }

    return (
        <div className="bg-ink-10 flex flex-1 flex-col p-6">
            {/* Header */}
            <div className="mb-4 flex items-center justify-between">
                <button
                    className="border-line-subtle text-app-text/70 hover:text-app-text cursor-pointer rounded border px-3 py-1 text-xs transition-colors"
                    onClick={() => nav(-1)}
                >
                    Back
                </button>
                <h1 className="text-lg font-bold tracking-widest">
                    BACKTEST RUN
                </h1>
                <span className="text-app-text/40 font-mono text-xs">
                    {detail.runId}
                </span>
            </div>

            {/* Summary stats */}
            <div className="grid grid-cols-2 gap-2 md:grid-cols-4">
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Initial Equity</p>
                    <p>{num(detail.initialEquity, 2)}</p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Final Equity</p>
                    <p>{num(detail.finalEquity, 2)}</p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Gross Profit</p>
                    <p className="text-accent-success">
                        +{num(detail.grossProfit, 2)}
                    </p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Gross Loss</p>
                    <p className="text-accent-danger-soft">
                        {num(detail.grossLoss, 2)}
                    </p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Wins / Losses</p>
                    <p>
                        {detail.wins} / {detail.losses}
                    </p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Avg Win</p>
                    <p>{num(detail.avgWin, 2)}</p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Avg Loss</p>
                    <p>{num(detail.avgLoss, 2)}</p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Expectancy</p>
                    <p>{num(detail.expectancy, 4)}</p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Max DD (abs)</p>
                    <p>{num(detail.maxDrawdownAbs, 2)}</p>
                </div>
                <div className="border-line-subtle bg-ink-80 rounded border p-2 text-sm">
                    <p className="text-app-text/50 text-xs">Candles</p>
                    <p>
                        {detail.candlesProcessed} / {detail.candlesLoaded}
                    </p>
                </div>
            </div>

            {/* Stacked line charts — shared time axis, single crosshair */}
            {detail.equityCurve.length > 0 && (
                <div className="border-line-subtle bg-ink-80 z-2 mt-4 overflow-hidden rounded border">
                    <LineChartsContainer
                        startTime={chartStart}
                        endTime={chartEnd}
                        onTimeRangeChange={handleTimeRangeChange}
                    >
                        {({ chartWidth, crosshairX }) => (
                            <>
                                <LineChart
                                    series={equitySeries}
                                    startTime={chartStart}
                                    endTime={chartEnd}
                                    crosshairX={crosshairX}
                                    chartWidth={chartWidth}
                                    height={180}
                                    label="Equity / Balance"
                                />
                                <div className="border-line-subtle border-t" />
                                <LineChart
                                    series={upnlSeries}
                                    startTime={chartStart}
                                    endTime={chartEnd}
                                    crosshairX={crosshairX}
                                    chartWidth={chartWidth}
                                    height={120}
                                    label="Unrealised PnL"
                                    zeroLine
                                />
                                {cumulativePnlSeries.length > 0 && (
                                    <>
                                        <div className="border-line-subtle border-t" />
                                        <LineChart
                                            series={cumulativePnlSeries}
                                            startTime={chartStart}
                                            endTime={chartEnd}
                                            crosshairX={crosshairX}
                                            chartWidth={chartWidth}
                                            height={120}
                                            label="Cumulative PnL"
                                            zeroLine
                                        />
                                    </>
                                )}
                            </>
                        )}
                    </LineChartsContainer>
                </div>
            )}

            {/* Trades table */}
            <div className="z-3 mt-4 min-h-0 flex-1 overflow-auto">
                <p className="text-app-text/50 mb-2 text-xs uppercase">
                    Trades ({detail.trades.length})
                </p>
                <table className="w-full min-w-[760px] text-left text-xs">
                    <thead className="text-app-text/60 border-line-subtle border-b uppercase">
                        <tr>
                            <th className="py-2 pr-4 text-left">Side</th>
                            <th className="py-2 pr-4 text-right">Open</th>
                            <th className="py-2 pr-4 text-right">Close</th>
                            <th className="py-2 pr-4 text-right">PnL</th>
                            <th className="py-2 pr-4 text-right">Size</th>
                            <th className="py-2 pr-4 text-right">Fee</th>
                            <th className="py-2 pr-4 text-right">Funding</th>
                            <th className="py-2 text-right">
                                Open Time - Close Time
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        {detail.trades.length === 0 ? (
                            <tr>
                                <td
                                    colSpan={8}
                                    className="text-app-text/45 p-3 text-center"
                                >
                                    No trades in this run.
                                </td>
                            </tr>
                        ) : (
                            detail.trades.map((trade, idx) => (
                                <tr
                                    key={idx}
                                    className="border-line-subtle border-b last:border-b-0"
                                >
                                    <td
                                        className={`py-2 pr-4 font-semibold uppercase ${
                                            trade.side === "long"
                                                ? "text-accent-success-strong"
                                                : "text-accent-danger"
                                        }`}
                                    >
                                        {trade.side}
                                    </td>
                                    <td className="py-2 pr-4 text-right">
                                        {formatPrice(trade.open.price)}
                                    </td>
                                    <td className="py-2 pr-4 text-right">
                                        {formatPrice(trade.close.price)}
                                    </td>
                                    <td
                                        className={`py-2 pr-4 text-right ${
                                            trade.pnl >= 0
                                                ? "text-accent-success"
                                                : "text-accent-danger-soft"
                                        }`}
                                    >
                                        {num(trade.pnl, 2)}$
                                    </td>
                                    <td className="py-2 pr-4 text-right">
                                        {num(trade.size, 4)}
                                    </td>
                                    <td className="py-2 pr-4 text-right">
                                        {num(trade.fees, 4)}$
                                    </td>
                                    <td className="py-2 pr-4 text-right">
                                        {num(trade.funding, 4)}$
                                    </td>
                                    <td className="py-2 text-right">
                                        {formatUTC(trade.open.time)} -{" "}
                                        {formatUTC(trade.close.time)}
                                    </td>
                                </tr>
                            ))
                        )}
                    </tbody>
                </table>
            </div>

            {/* Snapshots count */}
            {detail.snapshots.length > 0 && (
                <div className="border-line-subtle bg-ink-80 mt-4 rounded border p-3">
                    <p className="text-app-text/50 text-xs uppercase">
                        Snapshots ({detail.snapshots.length}) — indicators &
                        position state per event
                    </p>
                </div>
            )}
        </div>
    );
}
