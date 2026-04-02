import { API_URL } from "../consts";
import type { BacktestRunEntry, BacktestResultDetail } from "../types";

function authHeaders(token: string | null): Record<string, string> {
    const h: Record<string, string> = {};
    if (token) h["Authorization"] = `Bearer ${token}`;
    return h;
}

export interface BacktestHistoryParams {
    asset?: string;
    strategyId?: string;
    limit?: number;
    offset?: number;
}

export async function fetchBacktestHistory(
    token: string | null,
    params?: BacktestHistoryParams
): Promise<BacktestRunEntry[]> {
    const search = new URLSearchParams();
    if (params?.asset) search.set("asset", params.asset);
    if (params?.strategyId) search.set("strategy_id", params.strategyId);
    if (params?.limit != null) search.set("limit", String(params.limit));
    if (params?.offset != null) search.set("offset", String(params.offset));
    const qs = search.toString();
    const url = `${API_URL}/backtest/history${qs ? `?${qs}` : ""}`;

    const res = await fetch(url, { headers: authHeaders(token) });
    if (!res.ok)
        throw new Error(`Failed to fetch backtest history (${res.status})`);
    return res.json();
}

export async function fetchBacktestResult(
    token: string | null,
    runId: string
): Promise<BacktestResultDetail> {
    const res = await fetch(`${API_URL}/backtest/history/${runId}`, {
        headers: authHeaders(token),
    });
    if (!res.ok)
        throw new Error(`Failed to fetch backtest result (${res.status})`);
    return res.json();
}

export async function deleteBacktestRun(
    token: string | null,
    runId: string
): Promise<void> {
    const res = await fetch(`${API_URL}/backtest/history/${runId}`, {
        method: "DELETE",
        headers: authHeaders(token),
    });
    if (!res.ok)
        throw new Error(`Failed to delete backtest run (${res.status})`);
}
