import React, { useState, useEffect, useRef } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Plus, Ban, Power, Pause, Play, X, AlertTriangle, Trash2, PauseCircle } from 'lucide-react';
import MarketCard from './MarketCard';
import { AddMarket } from './AddMarket';
import type { MarketInfo, Message, assetPrice, MarketTradeInfo, assetMargin, indicatorData } from '../types';

// ---------- MarketsPage (Brutalist)
export default function MarketsPage() {
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const errRef = useRef<NodeJS.Timeout | null>(null);

  const [markets, setMarkets] = useState<MarketInfo[]>([]);
  const [totalMargin, setTotalMargin] = useState(0);
  const [marketToRemove, setMarketToRemove] = useState<string | null>(null);
  const [marketToToggle, setMarketToToggle] = useState<string | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectRef = useRef<number>();

  useEffect(() => {
    if (wsRef.current) return;
    const connect = () => {
      const ws = new WebSocket('ws://localhost:8090/ws');
      wsRef.current = ws;
      ws.onopen = () => load_session();
      ws.onmessage = (event: MessageEvent) => {
        const payload = JSON.parse(event.data) as Message;
        if ('confirmMarket' in payload) {
          setMarkets(prev => [
            ...prev,
            { ...payload.confirmMarket, trades: Array.isArray(payload.confirmMarket.trades) ? payload.confirmMarket.trades : [] },
          ]);
        } else if ('updatePrice' in payload) {
          const [asset, price] = payload.updatePrice as assetPrice;
          setMarkets(prev => prev.map(m => (m.asset === asset ? { ...m, price } : m)));
        } else if ('newTradeInfo' in payload) {
          const { asset, info } = payload.newTradeInfo as MarketTradeInfo;
          setMarkets(prev => prev.map(m => (m.asset === asset ? { ...m, trades: [...(Array.isArray(m.trades) ? m.trades : []), info], pnl: (m.pnl += info.pnl) } : m)));
        } else if ('updateTotalMargin' in payload) {
          setTotalMargin(payload.updateTotalMargin);
        } else if ('updateMarketMargin' in payload) {
          const [asset, margin] = payload.updateMarketMargin as assetMargin;
          setMarkets(prev => prev.map(m => (m.asset === asset ? { ...m, margin } : m)));
        } else if ('updateIndicatorValues' in payload) {
          const { asset, data } = payload.updateIndicatorValues as { asset: string; data: indicatorData[] };
          setMarkets(prev => prev.map(m => (m.asset === asset ? { ...m, indicators: data } : m)));
        } else if ('userError' in payload) {
          setErrorMsg(payload.userError);
          if (errRef.current) clearTimeout(errRef.current);
          errRef.current = setTimeout(() => setErrorMsg(null), 5000);
        } else if ('loadSession' in payload) {
          setMarkets(payload.loadSession);
        }
      };
      ws.onerror = err => console.error('WebSocket error', err);
      ws.onclose = () => {
        reconnectRef.current = window.setTimeout(connect, 1000);
      };
    };
    connect();
    return () => {
      if (reconnectRef.current) clearTimeout(reconnectRef.current);
      wsRef.current?.close();
    };
  }, []);

  const remove_market = async (asset: string) => {
    await fetch('http://localhost:8090/command', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ removeMarket: asset.toUpperCase() }),
    });
  };
  const toggle_market = async (asset: string) => {
    await fetch('http://localhost:8090/command', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ toggleMarket: asset.toUpperCase() }),
    });
  };
  const load_session = async () => {
    await fetch('http://localhost:8090/command', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ getSession: null }),
    });
  };

  const handleConfirmToggle = (asset: string, isPaused: boolean) => {
    if (isPaused) {
      toggle_market(asset);
      setMarkets(prev => prev.map(m => (m.asset === asset ? { ...m, is_paused: false } : m)));
    } else setMarketToToggle(asset);
  };
  const handleTogglePause = (asset: string) => {
    toggle_market(asset);
    setMarkets(prev => prev.map(m => (m.asset === asset ? { ...m, is_paused: true } : m)));
    setMarketToToggle(null);
  };
  const handleRemove = (asset: string) => {
    remove_market(asset);
    setMarkets(prev => prev.filter(m => m.asset !== asset));
    setMarketToRemove(null);
  };
  const closeAll = async () => {
    await fetch('http://localhost:8090/command', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ closeAll: null }),
    });
  };
  const pauseAll = async () => {
    await fetch('http://localhost:8090/command', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ pauseAll: null }),
    });
  };

  return (
    <div className="min-h-screen bg-[linear-gradient(180deg,#0a0a0a_0%,#0f0f0f_100%)] text-white">
      {/* Brutalist Header */}
      <div className="sticky top-0 z-40 border-b border-white/20 bg-black">
        <div className="mx-auto max-w-7xl p-4">
          <div className="grid grid-cols-2 md:grid-cols-3 items-end">
            <div>
              <div className="text-[11px] uppercase text-white/60 tracking-widest">Console</div>
              <h1 className="-mt-1 text-4xl font-black leading-none">KWANT//MARKETS</h1>
            </div>
            <div className="hidden md:flex items-center justify-center gap-3">
              <div className="border border-white/30 px-3 py-2 font-mono text-sm">
                MARGIN <span className="ml-2 tabular-nums">{totalMargin.toFixed(2)}</span>
              </div>
              {markets.length !== 0 && (
                <button onClick={() => setShowAdd(true)} className="border border-lime-400 bg-lime-400/10 px-3 py-2 text-lime-200 hover:bg-lime-400/20">
                  <div className="flex items-center gap-2"><Plus className="h-4 w-4" /><span>ADD</span></div>
                </button>
              )}
            </div>
            <div className="flex items-center justify-end gap-2">
              <button className="border border-red-500 bg-red-600/20 px-3 py-2 hover:bg-red-600/30" onClick={() => { closeAll(); setMarkets([]); }}>
                <div className="flex items-center gap-2"><Power className="h-4 w-4" /><span>KILL ALL</span></div>
              </button>
              <button className="border border-amber-500 bg-amber-500/20 px-3 py-2 hover:bg-amber-500/30" onClick={() => { pauseAll(); markets.forEach(m => (m.is_paused = true)); }}>
                <div className="flex items-center gap-2"><Pause className="h-4 w-4" /><span>PAUSE ALL</span></div>
              </button>
            </div>
          </div>

          {/* Ticker strip */}
          <div className="mt-4 overflow-hidden border-y border-white/10">
            <div className="whitespace-nowrap [animation:scroll_30s_linear_infinite] text-sm">
              <span className="mx-6 text-white/70">SYSTEM STATUS: <b className="text-lime-300">ONLINE</b></span>
              <span className="mx-6 text-white/70">SESSION: REAL‑TIME</span>
              <span className="mx-6 text-white/70">RISK: MANAGED</span>
              <span className="mx-6 text-white/70">UI MODE: BRUTALIST</span>
              <span className="mx-6 text-white/70">TRADES STREAMING…</span>
            </div>
          </div>
        </div>
      </div>

      {/* Empty state */}
      {markets.length === 0 && (
        <div className="mx-auto max-w-4xl px-4 py-24">
          <div className="border-2 border-dashed border-white/30 p-10">
            <div className="text-[11px] uppercase text-white/60">Initialize</div>
            <div className="mt-1 flex items-end justify-between">
              <h2 className="text-5xl font-black">NO MARKETS</h2>
              <button onClick={() => setShowAdd(true)} className="border border-lime-400 bg-lime-400/10 px-4 py-2 text-lime-200 hover:bg-lime-400/20">
                <div className="flex items-center gap-2"><Plus className="h-4 w-4" /><span>ADD MARKET</span></div>
              </button>
            </div>
            <p className="mt-3 max-w-prose text-white/70">Add at least one market to begin streaming quotes and executing strategies.</p>
          </div>
        </div>
      )}

      {/* Markets grid */}
      {markets.length > 0 && (
        <div className="mx-auto max-w-7xl px-4 py-10">
          <div className="grid grid-cols-1 gap-6 md:grid-cols-2 xl:grid-cols-3">
            {markets.map(m => (
              <MarketCard key={m.asset} market={m} onTogglePause={() => handleConfirmToggle(m.asset, m.is_paused)} onRemove={() => setMarketToRemove(m.asset)} />
            ))}
          </div>
        </div>
      )}

      {/* Error banner */}
      <AnimatePresence>
        {errorMsg && (
          <motion.div initial={{ y: -16, opacity: 0 }} animate={{ y: 0, opacity: 1 }} exit={{ y: -16, opacity: 0 }} className="fixed left-0 right-0 top-16 z-50">
            <div className="mx-auto max-w-3xl border border-red-500 bg-[#2a0d0d] px-4 py-3 text-red-200">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2"><AlertTriangle className="h-4 w-4" /><span className="text-sm">{errorMsg}</span></div>
                <button onClick={() => setErrorMsg(null)} className="border border-red-400/50 px-2 py-1 text-xs hover:bg-red-500/20">DISMISS</button>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Add Market Modal */}
      <AnimatePresence>
        {showAdd && (
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="fixed inset-0 z-50">
            <div className="absolute inset-0 bg-black/70" onClick={() => setShowAdd(false)} />
            <motion.div initial={{ y: 30, opacity: 0 }} animate={{ y: 0, opacity: 1 }} exit={{ y: 10, opacity: 0 }} className="relative mx-auto mt-24 w-full max-w-2xl border border-white/20 bg-[#0d0d0d] p-6">
              <div className="flex items-center justify-between border-b border-white/20 pb-3">
                <div className="text-[11px] uppercase text-white/60">Create</div>
                <button onClick={() => setShowAdd(false)} className="border border-white/30 p-1 hover:bg-white/10"><X className="h-4 w-4" /></button>
              </div>
              <div className="pt-4">
                <AddMarket onClose={() => setShowAdd(false)} totalMargin={totalMargin} />
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Confirm Remove */}
      <AnimatePresence>
        {marketToRemove && (
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="fixed inset-0 z-50">
            <div className="absolute inset-0 bg-black/70" onClick={() => setMarketToRemove(null)} />
            <motion.div initial={{ y: 30, opacity: 0 }} animate={{ y: 0, opacity: 1 }} exit={{ y: 10, opacity: 0 }} className="relative mx-auto mt-28 w-full max-w-md border border-red-500 bg-[#1a0c0c] p-6">
              <div className="text-[11px] uppercase text-red-300">Danger</div>
              <h3 className="mt-1 text-2xl font-black">REMOVE {marketToRemove} ?</h3>
              <p className="mt-1 text-red-200/80">Ongoing bot trades will be closed.</p>
              <div className="mt-6 flex justify-end gap-2">
                <button className="border border-white/30 px-3 py-2 hover:bg-white/10" onClick={() => setMarketToRemove(null)}>CANCEL</button>
                <button className="border border-red-500 bg-red-600/20 px-3 py-2 hover:bg-red-600/30" onClick={() => handleRemove(marketToRemove)}>YES</button>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Confirm Pause */}
      <AnimatePresence>
        {marketToToggle && (
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="fixed inset-0 z-50">
            <div className="absolute inset-0 bg-black/70" onClick={() => setMarketToToggle(null)} />
            <motion.div initial={{ y: 30, opacity: 0 }} animate={{ y: 0, opacity: 1 }} exit={{ y: 10, opacity: 0 }} className="relative mx-auto mt-28 w-full max-w-md border border-amber-500 bg-[#1a160c] p-6">
              <div className="text-[11px] uppercase text-amber-300">Control</div>
              <h3 className="mt-1 text-2xl font-black">PAUSE {marketToToggle} ?</h3>
              <p className="mt-1 text-amber-200/80">Ongoing bot trades will be closed.</p>
              <div className="mt-6 flex justify-end gap-2">
                <button className="border border-white/30 px-3 py-2 hover:bg-white/10" onClick={() => setMarketToToggle(null)}>CANCEL</button>
                <button className="border border-amber-500 bg-amber-600/20 px-3 py-2 hover:bg-amber-600/30" onClick={() => handleTogglePause(marketToToggle)}>YES</button>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* keyframes for ticker */}
      <style>{`@keyframes scroll{0%{transform:translateX(0)}100%{transform:translateX(-50%)}}`}</style>
    </div>
  );
}

