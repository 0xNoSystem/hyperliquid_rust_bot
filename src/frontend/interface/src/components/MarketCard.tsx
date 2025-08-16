import React from 'react';
import { motion } from 'framer-motion';
import { Play, Pause, Trash2 } from 'lucide-react';
import type { MarketInfo } from '../types';
import { indicatorLabels, indicatorColors, decompose, get_value, fromTimeFrame } from '../types';

interface MarketCardProps {
  market: MarketInfo;
  onTogglePause: (asset: string) => void;
  onRemove: (asset: string) => void;
}

const formatPrice = (n: number) => (n < 1 ? n.toFixed(4) : n.toFixed(2));

const PnlBar: React.FC<{ pnl: number }> = ({ pnl }) => {
  const pct = Math.max(-100, Math.min(100, pnl));
  const pos = pnl >= 0;
  return (
    <div className="border border-white/20 p-1">
      <div className="h-6 w-full bg-black">
        <div
          className={`${pos ? 'bg-lime-400' : 'bg-rose-500'}`}
          style={{ width: `${Math.abs(pct)}%`, height: '100%' }}
        />
      </div>
      <div className="mt-1 text-right font-mono text-xs tabular-nums {pos ? 'text-lime-300' : 'text-rose-300'}">{pnl >= 0 ? '+' : ''}{pnl.toFixed(2)}</div>
    </div>
  );
};

const MarketCard: React.FC<MarketCardProps> = ({ market, onTogglePause, onRemove }) => {
  const { asset, price, lev, margin, params, pnl, is_paused, indicators } = market;
  const { strategy } = params;
  const { risk, style, stance } = strategy.custom;

  return (
    <motion.div whileHover={{ scale: 1.01 }} className="group border border-white/25 bg-[#0d0d0d] p-4">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <div className="text-[10px] uppercase text-white/60">Asset</div>
          <div className="-mt-1 flex items-baseline gap-3">
            <h2 className="text-4xl font-black tracking-tight">{asset}</h2>
            <span className={`border px-2 py-0.5 text-[10px] uppercase ${is_paused ? 'border-amber-400 text-amber-300' : 'border-lime-400 text-lime-300'}`}>{is_paused ? 'PAUSED' : 'LIVE'}</span>
          </div>
        </div>
        <div className="flex gap-2">
          <button onClick={() => onTogglePause(asset)} className="border border-white/30 px-3 py-2 hover:bg-white/10" title="Toggle">
            {is_paused ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
          </button>
          <button onClick={() => onRemove(asset)} className="border border-white/30 px-3 py-2 hover:bg-rose-600/20" title="Remove">
            <Trash2 className="h-4 w-4" />
          </button>
        </div>
      </div>

      {/* Price + Lev */}
      <div className="mt-3 grid grid-cols-3 gap-3">
        <div>
          <div className="text-[10px] uppercase text-white/60">Price</div>
          <div className="font-mono text-xl tabular-nums">${formatPrice(price)}</div>
        </div>
        <div>
          <div className="text-[10px] uppercase text-white/60">Leverage</div>
          <div className="font-mono text-xl">{lev}×</div>
        </div>
        <div>
          <div className="text-[10px] uppercase text-white/60">Margin</div>
          <div className="font-mono text-xl">${margin.toFixed(2)}</div>
        </div>
      </div>

      {/* PnL Bar */}
      <div className="mt-4">
        <div className="text-[10px] uppercase text-white/60">PnL</div>
        <PnlBar pnl={pnl} />
      </div>

      {/* Indicators as hard tags */}
      <div className="mt-4 flex flex-wrap gap-2">
        {indicators.map((data, i) => {
          const { kind, timeframe, value } = decompose(data);
          const kindKey = Object.keys(kind)[0] as keyof typeof indicatorColors;
          return (
            <span
              key={i}
              title={get_value(value)}
              className={`border border-white/30 px-2 py-1 text-[11px] ${indicatorColors[kindKey]} bg-black`}
            >
              {indicatorLabels[kindKey] || (kindKey as string)} // {fromTimeFrame(timeframe)}
            </span>
          );
        })}
      </div>

      {/* Strategy row */}
      <div className="mt-4 grid grid-cols-3 gap-3 border-t border-white/15 pt-3 text-xs">
        <div>
          <div className="text-[10px] uppercase text-white/60">Strategy</div>
          <div>{style} / {stance}</div>
        </div>
        <div>
          <div className="text-[10px] uppercase text-white/60">Risk</div>
          <div>{risk}</div>
        </div>
        <div className="text-right">
          <div className="text-[10px] uppercase text-white/60">Trend Following</div>
          <div>{strategy.custom.followTrend ? 'Yes' : 'No'}</div>
        </div>
      </div>
    </motion.div>
  );
};

export default MarketCard;
