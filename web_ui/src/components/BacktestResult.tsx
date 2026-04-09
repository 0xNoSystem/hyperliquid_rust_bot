import { useNavigate } from "react-router-dom";
import { fromTimeFrame, formatPrice, num } from "../types";
import type { BacktestResult as BacktestResultType } from "../types";
import { formatUTC } from "../chart/utils";

function formatUtcMinute(ts: number): string {
    return new Date(ts).toISOString().slice(0, 16).replace("T", " ") + " UTC";
}

interface BacktestResultProps {
    result: BacktestResultType;
    /** Hide the detail button (e.g. when already on detail page) */
    hideDetailButton?: boolean;
}

export default function BacktestResult({
    result,
    hideDetailButton,
}: BacktestResultProps) {
    const nav = useNavigate();

    return (
        <div className="border-line-muted bg-ink-80 mx-auto mt-2 flex min-h-0 w-full flex-1 flex-col rounded border p-4 text-sm">
            <div className="flex items-center justify-between">
                <p className="text-app-text/70 font-mono text-xs">
                    Run ID: {result.runId}
                </p>
                {!hideDetailButton && (
                    <button
                        className="border-line-subtle text-app-text/70 hover:text-app-text hover:border-line-muted cursor-pointer rounded border px-2 py-1 text-xs transition-colors"
                        onClick={() =>
                            nav(`/backtest/run/${result.runId}`, {
                                state: { result },
                            })
                        }
                    >
                        See More
                    </button>
                )}
            </div>

            <div className="border-line-subtle mt-3 rounded border p-3">
                <p className="text-app-text/50 text-xs uppercase">Run Config</p>
                <div className="mt-2 grid grid-cols-1 gap-2 text-xs md:grid-cols-2 lg:grid-cols-3">
                    <div>
                        <p className="text-app-text/50">Asset</p>
                        <p className="text-app-text">{result.config.asset}</p>
                    </div>
                    <div>
                        <p className="text-app-text/50">Source</p>
                        <p className="text-app-text">
                            {result.config.source.exchange.toUpperCase()} /{" "}
                            {result.config.source.market.toUpperCase()} /{" "}
                            {result.config.source.quoteAsset}
                        </p>
                    </div>
                    <div>
                        <p className="text-app-text/50">Strategy</p>
                        <p className="text-app-text">
                            {result.config.strategyId}
                        </p>
                    </div>
                    <div>
                        <p className="text-app-text/50">Resolution</p>
                        <p className="text-app-text">
                            {fromTimeFrame(result.config.resolution)}
                        </p>
                    </div>
                    <div>
                        <p className="text-app-text/50">Window (UTC)</p>
                        <p className="text-app-text">
                            {formatUtcMinute(result.config.startTime)} -{" "}
                            {formatUtcMinute(result.config.endTime)}
                        </p>
                    </div>
                    <div>
                        <p className="text-app-text/50">Margin / Leverage</p>
                        <p className="text-app-text">
                            {num(result.config.margin, 2)} / {result.config.lev}
                            x
                        </p>
                    </div>
                    <div>
                        <p className="text-app-text/50">Fees (bps)</p>
                        <p className="text-app-text">
                            taker {result.config.takerFeeBps} / maker{" "}
                            {result.config.makerFeeBps}
                        </p>
                    </div>
                    <div>
                        <p className="text-app-text/50">Funding (bps / 8h)</p>
                        <p className="text-app-text">
                            {num(result.config.fundingRateBpsPer8h, 4)}
                        </p>
                    </div>
                    <div>
                        <p className="text-app-text/50">Snapshot Interval</p>
                        <p className="text-app-text">
                            {result.config.snapshotIntervalCandles} candles
                        </p>
                    </div>
                </div>
            </div>

            <div className="mt-3 grid grid-cols-2 gap-2 md:grid-cols-4">
                <div className="border-line-subtle rounded border p-2">
                    <p className="text-app-text/50 text-xs">Trades</p>
                    <p>{result.summary.totalTrades}</p>
                </div>
                <div className="border-line-subtle rounded border p-2">
                    <p className="text-app-text/50 text-xs">Net PnL</p>
                    <p
                        className={
                            result.summary.netPnl >= 0
                                ? "text-accent-success"
                                : "text-accent-danger-soft"
                        }
                    >
                        {result.summary.netPnl >= 0 ? "+" : ""}
                        {num(result.summary.netPnl, 2)}
                    </p>
                </div>
                <div className="border-line-subtle rounded border p-2">
                    <p className="text-app-text/50 text-xs">Return %</p>
                    <p>{num(result.summary.returnPct, 2)}%</p>
                </div>
                <div className="border-line-subtle rounded border p-2">
                    <p className="text-app-text/50 text-xs">Sharpe</p>
                    <p>
                        {result.summary.sharpeRatio == null
                            ? "—"
                            : num(result.summary.sharpeRatio, 3)}
                    </p>
                </div>
                <div className="border-line-subtle rounded border p-2">
                    <p className="text-app-text/50 text-xs">Win Rate</p>
                    <p>{num(result.summary.winRatePct, 2)}%</p>
                </div>
                <div className="border-line-subtle rounded border p-2">
                    <p className="text-app-text/50 text-xs">Profit Factor</p>
                    <p>
                        {result.summary.profitFactor == null
                            ? "—"
                            : num(result.summary.profitFactor, 2)}
                    </p>
                </div>
                <div className="border-line-subtle rounded border p-2">
                    <p className="text-app-text/50 text-xs">Max DD %</p>
                    <p>{num(result.summary.maxDrawdownPct, 2)}%</p>
                </div>
                <div className="border-line-subtle rounded border p-2">
                    <p className="text-app-text/50 text-xs">Candles</p>
                    <p>{result.candlesProcessed}</p>
                </div>
            </div>

            <div className="mt-4 min-h-0 flex-1 overflow-auto">
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
                        {result.trades.length === 0 ? (
                            <tr>
                                <td
                                    colSpan={8}
                                    className="text-app-text/45 p-3 text-center"
                                >
                                    No trades in this run.
                                </td>
                            </tr>
                        ) : (
                            result.trades.map((trade, idx) => (
                                <tr
                                    key={`${result.runId}-${idx}`}
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
        </div>
    );
}
