import React from 'react';
import { motion } from 'framer-motion';
import { Pause, Play, Trash2, ExternalLink } from 'lucide-react';
import type { MarketInfo } from '../types';
import { indicatorLabels, indicatorColors, decompose, get_value,get_params, fromTimeFrame } from '../types';
import LoadingDots from './Loading';

interface MarketCardProps {
  market: MarketInfo;
  onTogglePause: (asset: string) => void;
  onRemove: (asset: string) => void;
}

const formatPrice = (n: number) => {
  if (n > 1 && n < 2) return n.toFixed(4);
  if (n < 1) return n.toFixed(6);
  return n.toFixed(2);
};

const PnlBar: React.FC<{ pnl: number }> = ({ pnl }) => {
  const w = Math.min(100, Math.abs(pnl));
  const pos = pnl != null ? pnl >= 0 : true;
  return (
    <div className="rounded-md border border-white/10 bg-black p-1">
      <div className="h-1.5 w-full bg-white/5">
        <div
          className={pos ? 'bg-orange-400' : 'bg-rose-500'}
          style={{ width: `${w}%`, height: '100%' }}
        />
      </div>
      <div
        className={`mt-1 text-right font-mono text-[11px] tabular-nums ${
          pos ? 'text-orange-300' : 'text-rose-300'
        }`}
      >
        {pos ? '+' : ''}
        {pnl != null ? pnl.toFixed(2) : 0.00}
      </div>
    </div>
  );
};

const MarketCard: React.FC<MarketCardProps> = ({ market, onTogglePause, onRemove }) => {
  const { asset, state, price, lev, margin, params, pnl, isPaused, indicators } = market;
  const { strategy } = params;
  const { risk, style, stance } = strategy.custom;

  const loading = state === 'Loading';

  return (
    <motion.div
      whileHover={{ y: -2 }}
      className="group rounded-md border border-white/10 bg-[#111316] p-4 shadow-[0_2px_0_rgba(255,255,255,0.03),_0_12px_24px_rgba(0,0,0,0.35)]"
    >
      {/* Head */}
      <div className="mb-3 flex items-start justify-between">
        <div>
          <div className="text-[10px] uppercase text-white/50">Asset</div>
          <div className="-mt-0.5 flex items-baseline gap-3">
            <h2 className="text-3xl font-semibold tracking-tight">
              {asset}
              <a
                href={`https://app.hyperliquid.xyz/trade/${asset}`}
                target="_blank"
                rel="noopener noreferrer"
                className="hidden md:inline-flex items-center gap-2 rounded-md border border-white/10 bg-[#111316] ml-3 text-[12px] text-white hover:bg-white/5"
              >
                <ExternalLink className="h-3.5 w-3.5 text-orange-400" />
              </a>
            </h2>
            <span
              className={`relative bottom-1 rounded-md px-2 py-0.5 text-[10px] uppercase ${
                isPaused
                  ? 'border border-amber-400/60 text-amber-300'
                  : 'border border-orange-500/60 text-orange-300'
              }`}
            >
              {loading ? 'Loading' : isPaused ? 'Paused' : 'Live'}
            </span>
          </div>
          <div className="mt-1 font-mono text-sm text-white/70">
            {loading || lev == null ? <LoadingDots /> : `${lev}×`}
          </div>
        </div>


        {loading ? null :
        <div className="flex gap-2">
          <button
            onClick={() => onTogglePause(asset)}
            className="grid h-9 w-9 place-items-center rounded-md border border-white/10 bg-white/[0.04] hover:bg-white/10"
            title="Toggle"
          >
            {isPaused ? (
              <Play className="h-4 w-4 text-orange-300" />
            ) : (
              <Pause className="h-4 w-4 text-amber-300" />
            )}
          </button>
          <button
            onClick={() => onRemove(asset)}
            className="grid h-9 w-9 place-items-center rounded-md border border-white/10 bg-white/[0.04] hover:bg-rose-600/20"
            title="Remove"
          >
            <Trash2 className="h-4 w-4 text-rose-300" />
          </button>
        </div>
        }
      </div>

      {/* Metrics */}
      <div className="grid grid-cols-3 gap-3">
        <div>
          <div className="text-[10px] uppercase text-white/50">Price</div>
          <div className="font-mono text-xl tabular-nums">
            {loading || price == null || Math.abs(price) < 1e-8
                ? <LoadingDots />
            : `$${formatPrice(price)}`}
          </div>
        </div>
        <div>
          <div className="text-[10px] uppercase text-white/50">Leverage</div>
          <div className="font-mono text-xl">
            {loading || lev == null ? <LoadingDots /> : `${lev}×`}
          </div>
        </div>
        <div>
          <div className="text-[10px] uppercase text-white/50">Margin</div>
          <div className="font-mono text-xl">
            {loading || margin == null ? <LoadingDots /> : `$${margin.toFixed(2)}`}
          </div>
        </div>
      </div>

      {/* PnL */}
      <div className="mt-4">
        <div className="text-[10px] uppercase text-white/50">PnL</div>
        <PnlBar pnl={pnl} />
      </div>

      {/* Indicators */}
      <div className="mt-3 flex flex-wrap gap-2">
        {loading ? (
          <LoadingDots />
        ) : (
          indicators.map((data, i) => {
            const { kind, timeframe, value } = decompose(data);
            const kindKey = Object.keys(kind)[0] as keyof typeof indicatorColors;

            return (
                <div className={`flex flex-col rounded-md cursor-pointer border border-white/10 bg-white/5 px-2.5 py-1 text-[11px] ${indicatorColors[kindKey]}`}                 title={get_params(kind)}>
              <span
                key={i}
                >
                {indicatorLabels[kindKey] || (kindKey as string)} — {fromTimeFrame(timeframe)}
                </span>
                <span className="text-center font-bold text-base">{get_value(value)}</span>
              </div>
            );
          })
        )}
      </div>

      {/* Strategy */}
      <div className="mt-4 grid grid-cols-3 gap-3 border-t border-white/10 pt-3 text-xs">
        {loading ? (
          <div className="col-span-3 flex justify-center">
            <LoadingDots />
          </div>
        ) : (
          <>
            <div>
              <div className="text-[10px] uppercase text-white/50">Strategy</div>
              <div className="truncate text-white/90">
                {style} / {stance}
              </div>
            </div>
            <div>
              <div className="text-[10px] uppercase text-white/50">Risk</div>
              <div className="text-white/90">{risk}</div>
            </div>
            <div className="text-right">
              <div className="text-[10px] uppercase text-white/50">Trend Following</div>
              <div className="text-white/90">
                {strategy.custom.followTrend ? 'Yes' : 'No'}
              </div>
            </div>
          </>
        )}
      </div>
    </motion.div>
  );
};

export default MarketCard;

