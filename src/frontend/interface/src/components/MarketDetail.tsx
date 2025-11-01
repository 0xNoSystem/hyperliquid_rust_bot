// src/components/MarketDetail.tsx
// Alternative “Trading Terminal” layout — keyboard/terminal vibes, split panes, neon accents.
// Keeps the same backend interactions and batching behavior.

import React, { useMemo, useState, useCallback } from "react";
import { Link, useParams } from "react-router-dom";
import { useWebSocketContext } from "../context/WebSocketContext";
import TradingViewWidget from "./TradingViewWidget";
import { BackgroundFX } from "../components/BackgroundFX";
import { motion, AnimatePresence } from "framer-motion";

import {
  decompose,
  indicatorColors,
  indicatorLabels,
  indicatorParamLabels,
  fromTimeFrame,
  into,
} from "../types";
import type {
  IndicatorKind,
  IndexId,
  MarketInfo,
  TimeFrame,
  TradeInfo,
} from "../types";
import { ArrowLeft, Plus, Minus, Save, X } from "lucide-react";


const formatPrice = (n: number) => {
  if (n > 1 && n < 2) return n.toFixed(4);
  if (n < 1) return n.toFixed(6);
  return n.toFixed(2);
};

/* ====== TOKENS ====== */
const Rail      = "rounded-xl border border-white/10 bg-[#0A0D10]/90 p-4 backdrop-blur";
const Pane      = "rounded-xl border border-white/10 bg-[#0B0E12]/80";
const Head      = "px-4 py-3 border-b border-white/10 text-[11px] uppercase tracking-wide text-white/60";
const Body      = "p-4";
const Chart     = "";
const Kbd       = "px-1.5 py-0.5 rounded border border-white/15 bg-black/30 font-mono text-[11px] text-white/80";
const Input     = "w-full rounded-lg px-3 py-2 border border-white/10 bg-[#0F1318] text-white focus:outline-none focus:ring-2 focus:ring-emerald-400/30";
const Select    = "appearance-none w-full rounded-lg px-3 py-2 border border-white/10 bg-[#0F1318] text-white focus:outline-none focus:ring-2 focus:ring-emerald-700/30 ";
const BtnGhost  = "inline-flex items-center justify-center rounded-lg border border-white/10 bg-white/5 px-3 py-2 hover:bg-white/10";
const BtnOK     = "inline-flex items-center justify-center rounded-lg border border-emerald-500/40 bg-emerald-700/25 px-3 py-2 text-emerald-200 hover:bg-orange-700/35 hover:cursor-pointer hover:border-orange-600 hover:text-white";
const Chip      = "inline-flex items-center gap-2 rounded-md border border-white/10 bg-orange-400/10 px-2 py-1 text-[15px] hover:bg-orange-500 hover:cursor-pointer";
const GridCols  = "grid grid-cols-1 xl:grid-cols-[300px_minmax(0,1fr)_360px] gap-4 p-8";

function px(n: number) {
  if (n > 1 && n < 2) return n.toFixed(4);
  if (n < 1) return n.toFixed(6);
  return n.toFixed(2);
}
function num(n: number, d = 2) {
  return Number.isFinite(n) ? n.toFixed(d) : "—";
}
function PnlTicker({ pnl }: { pnl: number | null }) {
  if (pnl == null) return <span className="font-mono text-white/60">PnL —</span>;
  const pos = pnl >= 0;
  return (
    <span className={`font-mono text-lg tabular-nums ${pos ? "text-emerald-300" : "text-rose-300"}`}>
      {pos ? "+" : ""}
      {num(pnl, 2)}
    </span>
  );
}

type PendingEdit = { id: IndexId; edit: "add" | "remove" };
const kindKeys = Object.keys(indicatorParamLabels) as Array<keyof typeof indicatorParamLabels>;

export default function MarketDetail() {
  const { asset: routeAsset } = useParams<{ asset: string }>();
  const { markets, universe, sendCommand, requestToggleMarket, totalMargin} = useWebSocketContext();
  const [marketToToggle, setMarketToToggle] = useState<string | null>(null);

  const handleConfirmToggle = (asset: string, isPaused: boolean) => {
    if (isPaused) {
      requestToggleMarket(asset).catch((err) =>
        console.error("Toggle failed", err),
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


  const market = useMemo<MarketInfo | undefined>(
    () => markets.find((m) => m.asset === (routeAsset ?? "").toUpperCase()),
    [markets, routeAsset]
  );
  const meta = useMemo(
    () => universe.find((u) => u.name === market?.asset),
    [universe, market]
  );

  if (!market) {
    return (
      <div className="mx-auto max-w-7xl px-6 py-8 text-white">
        <Link to="/" className={BtnGhost}><ArrowLeft className="h-4 w-4 mr-2" />Back</Link>
        <div className={`${Pane} ${Body} mt-6`}>Market not found.</div>
      </div>
    );
  }

  /* ----- local state ----- */
  const [lev, setLev] = useState<number>(market.lev ?? 1);
  const [margin, setMargin] = useState<number>(market.margin ?? 0);

  // builder
  const [kindKey, setKindKey] = useState<string>("rsi");
  const [p1, setP1] = useState<number>(14);
  const [p2, setP2] = useState<number>(14);
  const [tfSym, setTfSym] = useState<string>("1m");

  // batch
  const [pending, setPending] = useState<PendingEdit[]>([]);

  const maxLev = meta?.maxLeverage ?? 1;
  const eqIndexId = (a: IndexId, b: IndexId) => JSON.stringify(a) === JSON.stringify(b);

  const sendMarketCmd = (asset: string, cmd: unknown) =>
    sendCommand({ marketComm: { asset: asset.toUpperCase(), cmd } });

  const buildKind = useCallback((): IndicatorKind => {
    switch (kindKey) {
      case "emaCross": return { emaCross: { short: p1, long: p2 } };
      case "smaOnRsi": return { smaOnRsi: { periods: p1, smoothing_length: p2 } };
      case "stochRsi": return { stochRsi: { periods: p1, k_smoothing: null, d_smoothing: null } };
      case "adx": return { adx: { periods: p1, di_length: p2 } };
      case "rsi": return { rsi: p1 };
      case "atr": return { atr: p1 };
      case "ema": return { ema: p1 };
      case "sma": return { sma: p1 };
      default: return { rsi: 14 };
    }
  }, [kindKey, p1, p2]);

  const queueAdd = (id: IndexId) =>
    setPending((prev) => {
      const i = prev.findIndex((e) => e.edit === "remove" && eqIndexId(e.id, id));
      if (i !== -1) { const cp = prev.slice(); cp.splice(i, 1); return cp; }
      if (prev.some((e) => e.edit === "add" && eqIndexId(e.id, id))) return prev;
      return [...prev, { id, edit: "add" }];
    });

  const queueRemove = (id: IndexId) =>
    setPending((prev) => {
      const i = prev.findIndex((e) => e.edit === "add" && eqIndexId(e.id, id));
      if (i !== -1) { const cp = prev.slice(); cp.splice(i, 1); return cp; }
      if (prev.some((e) => e.edit === "remove" && eqIndexId(e.id, id))) return prev;
      return [...prev, { id, edit: "remove" }];
    });

  const discardPending = () => setPending([]);
  const applyPending = async () => {
    if (pending.length === 0) return;
    await sendMarketCmd(market.asset, { editIndicators: pending });
    setPending([]);
  };

  const onSaveLev = async () => {
    const clamped = Math.max(1, Math.min(lev, maxLev));
    await sendMarketCmd(market.asset, { updateLeverage: clamped });
  };
  const onSaveMargin = async () => {
    await sendCommand({ manualUpdateMargin: [market.asset, Math.max(0, margin)] });
  };

  /* ====== UI LAYOUT: rail | center (chart & indicators) | inspector ====== */
  return (
      

    <div className="relative mx-auto max-w-[2300px] px-6 py-8 text-white font-mono">
      <div className="mb-4 flex items-center justify-between">
        <Link to="/" className={BtnGhost}><ArrowLeft className="h-4 w-4 mr-2" />Back</Link>
        <div className="flex items-center gap-3">
          <button
            onClick={() =>
                      handleConfirmToggle(market.asset, market.isPaused)
                    }
            className={Chip}>{market.isPaused ? "Paused" : "Live"}
          </button>
          <h1 className="text-[30px] tracking-widest">{market.asset} <span className="text-[15px] text-orange-400">{market.lev}x</span></h1>
        </div>
        <div className="flex items-center gap-2 text-[11px] text-white/60">
          <span className={Kbd}>A</span><span>Add ind</span>
          <span className={Kbd}>R</span><span>Rm ind</span>
          <span className={Kbd}>Ctrl</span>+<span className={Kbd}>Enter</span><span>Apply</span>
        </div>
      </div>

      <div className={GridCols}>
            <BackgroundFX intensity={0.1} />
        {/* LEFT RAIL — quick stats & knobs */}
        <aside className={Rail}>
          <div className="space-y-4">
            <div className="rounded-lg border border-white/10 bg-black/20 p-3">
              <div className="text-[10px] uppercase text-white/50">Price</div>
              <div className="mt-1 text-2xl">{market.price == null ? "—" : `$${px(market.price)}`}</div>
            </div>

            <div className="rounded-lg border border-white/10 bg-black/20 p-3">
              <div className="flex items-center justify-between">
                <div className="text-[10px] uppercase text-white/50">Total PnL</div>
                <div className="h-1 w-16 bg-white/10">
                  {/* mini bar (cosmetic) */}
                  <div
                    className={`h-full ${((market.pnl ?? 0) >= 0) ? "bg-emerald-400" : "bg-rose-400"}`}
                    style={{ width: `${Math.min(100, Math.abs(market.pnl ?? 0))}%` }}
                  />
                </div>
              </div>
              <div className="mt-1"><PnlTicker pnl={market.pnl ?? 0} /></div>
            </div>

            {/* Leverage stepper */}
            <div className="rounded-lg border border-white/10 bg-black/20 p-3">
              <div className="text-[10px] uppercase text-white/50">Leverage <strong className="text-[13px]">{market.lev}×</strong></div>
              <div className="mt-2 flex items-center gap-2">
                <button
                  className={BtnGhost}
                  onClick={() => setLev((v) => Math.max(1, v - 1))}
                  aria-label="dec lev"
                >
                  <Minus className="h-4 w-4" />
                </button>
                <input
                  type="number"
                  min={1}
                  max={maxLev}
                  value={lev}
                  onChange={(e) => setLev(Math.max(1, Math.min(maxLev, +e.target.value)))}
                  className={`${Input} text-center w-24`}
                />
                <button
                  className={BtnGhost}
                  onClick={() => setLev((v) => Math.min(maxLev, v + 1))}
                  aria-label="inc lev"
                >
                  <Plus className="h-4 w-4" />
                </button>
              </div>
              <div className="mt-2 text-[11px] text-white/50">Max: {maxLev}×</div>
              <button onClick={onSaveLev} className={`${BtnOK} mt-3 w-full`}>
                Apply Leverage
              </button>
            </div>

            {/* Margin */}
            <div className="rounded-lg border border-white/10 bg-black/20 p-3">
              <div className="text-[10px] uppercase text-white/50 cursor-pointer" onClick={() => setMargin(totalMargin + market.margin)}>Margin (MAX: {(totalMargin + market.margin).toFixed(2)}$)</div>
              <div className="mt-2 items-center gap-2">
              <div className="flex flex-col py-4">
              <input type="range" min={0} max={(totalMargin + market.margin).toFixed(3)} step={0.01} value={margin.toFixed(2)} onChange={e => setMargin(+e.target.value)} className="w-full h-2 bg-gray-200 cursor-pointer" />
                <div className="flex justify-between text-sm text-white mt-1">
                    <span>{margin.toFixed(2)}$</span>
                    <span>{(margin / (totalMargin + market.margin) * 100).toFixed(1)}%</span>
                </div>
                </div>
                <button onClick={onSaveMargin} className={BtnOK}>
                  Apply
                </button>
                </div>
            </div>

            {/* Strategy snapshot */}
            <div className="rounded-lg border border-white/10 bg-black/20 p-3 text-[12px] space-y-1">
              <div className="flex items-center justify-between">
                <span className="text-white/60">Style</span> <span className="text-white/90">{market.params.strategy.custom.style}</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-white/60">Stance</span>
                <span className="text-white/90">{market.params.strategy.custom.stance}</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-white/60">Risk</span>
                <span className="text-white/90">{market.params.strategy.custom.risk}</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-white/60">Follow</span>
                <span className="text-white/90">
                  {market.params.strategy.custom.followTrend ? "Yes" : "No"}
                </span>
              </div>
            </div>
          </div>
        </aside>

        {/* CENTER — chart area + active indicators + trades */}
        <main className="space-y-4">
          {/* Chart placeholder with scanlines */}
          <section className={`${Pane}`}>
            <div className={Head}>Chart</div>
            <div className={`${Chart} relative h-[60vh] `}>
                <TradingViewWidget
                    symbol={`CRYPTO:${market.asset}USD`}
                    interval="D"
                    theme="dark"
                />
            </div>
          </section>

          {/* Active indicators list */}
          <section className={`${Pane}`}>
            <div className={`${Head} flex items-center justify-between`}>
              <span>Active Indicators</span>
              <span className="text-white/40">Count: {market.indicators.length}</span>
            </div>
            <div className={`${Body} flex flex-wrap gap-2`}>
              {market.indicators.map((data, i) => {
                const { kind, timeframe } = decompose(data);
                const kindKey = Object.keys(kind)[0] as string;
                return (
                  <div
                    key={`${kindKey}-${fromTimeFrame(timeframe)}-${i}`}
                    className={`group flex items-center gap-2 rounded-lg border border-white/10 px-2.5 py-1 text-[11px] ${
                      indicatorColors[kindKey] || "bg-white/10"
                    }`}
                    title={JSON.stringify(kind)}
                  >
                    <span className="font-medium">
                      {indicatorLabels[kindKey] || kindKey} — {fromTimeFrame(timeframe)}
                    </span>
                    <button
                      className="rounded p-0.5 hover:bg-white/10"
                      onClick={() => queueRemove([kind, timeframe])}
                      title="Queue remove"
                    >
                      <X className="h-3.5 w-3.5" />
                    </button>
                  </div>
                );
              })}
            </div>
          </section>

          {/* Trades table */}
          <section className={`${Pane}`}>
            <div className={Head}>Trades</div>
            <div className={`${Body} overflow-x-auto`}>
              {(!market.trades || market.trades.length === 0) ? (
                <div className="text-sm text-white/60">No trades yet.</div>
              ) : (
                <table className="min-w-full text-[12px]">
                  <thead className="text-white/60">
                    <tr>
                      <th className="py-2 pr-4 text-left">Side</th>
                      <th className="py-2 pr-4 text-right">Open</th>
                      <th className="py-2 pr-4 text-right">Close</th>
                      <th className="py-2 pr-4 text-right">PnL</th>
                      <th className="py-2 pr-4 text-right">Fee</th>
                      <th className="py-2 text-right">OID</th>
                    </tr>
                  </thead>
                  <tbody>
                    {market.trades.map((t: TradeInfo, i: number) => (
                      <tr key={i} className="border-t border-white/10">
                        <td className={`py-2 pr-4 font-semibold uppercase ${t.isLong ? "text-green-500" : "text-red-500"}`}>
                            {t.isLong ? "Long" : "Short"}
                        </td>
                        <td className="py-2 pr-4 text-right">{formatPrice(t.open)}</td>
                        <td className="py-2 pr-4 text-right">{formatPrice(t.close)}</td>
                        <td className={`py-2 pr-4 text-right ${t.pnl >= 0 ? "text-emerald-300" : "text-rose-300"}`}>
                          {num(t.pnl, 2)$}
                        </td>
                        <td className="py-2 pr-4 text-right">{num(t.fee, 2)}$</td>
                        <td className="py-2 text-right">{t.oid[0]} / {t.oid[1]}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </section>
        </main>

        {/* RIGHT — Indicator builder + Pending batch */}
        <aside className="space-y-4">
          <section className={Pane}>
            <div className={Head}>Add Indicator</div>
            <div className={`${Body} grid grid-cols-2 gap-3`}>
              <div className="col-span-2">
                <label className="text-[10px] uppercase text-white/50">Kind</label>
                <select
                  className={Select}
                  value={kindKey}
                  onChange={(e) => { setKindKey(e.target.value); setP1(14); setP2(14); }}
                >
                  {kindKeys.map((k) => (
                    <option key={k} value={k} className="bg-[#0F1318] text-white">
                      {indicatorLabels[k] || k}
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label className="text-[10px] uppercase text-white/50">
                  {indicatorParamLabels[kindKey]?.[0] ?? "Param"}
                </label>
                <input
                  type="number"
                  className={Input}
                  value={p1}
                  onChange={(e) => setP1(+e.target.value)}
                />
              </div>

              {["emaCross", "smaOnRsi", "adx"].includes(kindKey) && (
                <div>
                  <label className="text-[10px] uppercase text-white/50">
                    {indicatorParamLabels[kindKey]?.[1] ?? "Param2"}
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
                <label className="text-[10px] uppercase text-white/50">Timeframe</label>
                <select className={Select} value={tfSym} onChange={(e) => setTfSym(e.target.value)}>
                  {["1m","3m","5m","15m","30m","1h","2h","4h","12h","1d","3d","w","m"].map((s) => (
                    <option key={s} value={s} className="bg-[#0F1318] text-white">
                      {s}
                    </option>
                  ))}
                </select>
              </div>

              <div className="col-span-2">
                <button
                  onClick={() => {
                    const id: IndexId = [buildKind(), (into(tfSym) as TimeFrame)];
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
            <div className={`${Head} flex items-center justify-between`}>
              <span>Pending Changes</span>
              <span className="text-white/40">{pending.length}</span>
            </div>
            <div className={`${Body}`}>
              {pending.length === 0 ? (
                <div className="text-[12px] text-white/50">No pending edits.</div>
              ) : (
                <>
                  <div className="mb-3 flex flex-wrap gap-2">
                    {pending.map((e, idx) => {
                      const [kind, tf] = e.id;
                      const k = Object.keys(kind)[0] as string;
                      return (
                        <div
                          key={idx}
                          className={`flex items-center gap-2 rounded-md border border-white/10 px-2 py-0.5 text-[11px] ${
                            e.edit === "add" ? "bg-emerald-900/35" : "bg-rose-900/35"
                          }`}
                        >
                          <span className="uppercase tracking-wide">{e.edit}</span>
                          <span>· {indicatorLabels[k] || k} — {fromTimeFrame(tf)}</span>
                          <button
                            className="rounded p-0.5 hover:bg-white/10"
                            onClick={() => setPending((prev) => prev.filter((_, i) => i !== idx))}
                            title="Remove from batch"
                          >
                            <X className="h-3.5 w-3.5" />
                          </button>
                        </div>
                      );
                    })}
                  </div>
                  <div className="flex gap-2">
                    <button onClick={discardPending} className={BtnGhost}>Discard</button>
                    <button onClick={applyPending} className={BtnOK}>
                      <Save className="h-4 w-4 mr-2" />
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
                Pause <span className="text-amber-300">{marketToToggle}</span>?
              </h3>
              <p className="mt-1 text-amber-200/80">
                This will close any ongoing trade initiated by the Bot.
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
                  onClick={() => handleTogglePause(marketToToggle!)}
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

