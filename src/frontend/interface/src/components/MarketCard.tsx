import React from 'react';
import { FaPlay, FaPause, FaTrash } from 'react-icons/fa';
import type {IndicatorKind, MarketInfo} from '../types'

interface MarketCardProps {
  market: MarketInfo;
  onTogglePause: (asset: string) => void;
  onRemove: (asset: string) => void;
}



const indicatorLabels: Record<string, string> = {
  rsi: 'RSI',
  smaOnRsi: 'SMA on RSI',
  stochRsi: 'Stoch RSI',
  adx: 'ADX',
  atr: 'ATR',
  ema: 'EMA',
  emaCross: 'EMA Cross',
  sma: 'SMA',
};

const indicatorColors: Record<string, string> = {
  rsi: 'bg-green-800 text-green-200',
  smaOnRsi: 'bg-indigo-800 text-indigo-200',
  stochRsi: 'bg-purple-800 text-purple-200',
  adx: 'bg-yellow-800 text-yellow-200',
  atr: 'bg-red-800 text-red-200',
  ema: 'bg-blue-800 text-blue-200',
  emaCross: 'bg-pink-800 text-pink-200',
  sma: 'bg-gray-800 text-gray-200',
};

const MarketCard: React.FC<MarketCardProps> = ({ market, onTogglePause, onRemove }) => {
  const { asset, price, lev, margin, pnl, is_paused, indicators } = market;
  const actionLabel = is_paused ? 'Resume' : 'Pause';
  const ActionIcon = is_paused ? FaPlay : FaPause;

  return (
    <div className="bg-[#1D1D1D] text-gray-100 rounded-2xl shadow-lg relative flex flex-col w-full max-w-lg mx-auto">
      {/* Action Buttons */}
      <div className="absolute top-4 right-4 flex space-x-3 mt-2">
        <button
          onClick={() => onTogglePause(asset)}
          className="flex items-center justify-center p-3 bg-indigo-900 hover:bg-indigo-500 rounded-full focus:outline-none"
          title={`${actionLabel} trading`}
        >
          <ActionIcon className="w-4 h-4" />
        </button>
        <button
          onClick={() => onRemove(asset)}
          className="flex items-center justify-center p-3 bg-gray e-600 hover:bg-red-500 hover:cursor-pointer rounded-full focus:outline-none"
          title="Remove market"
        >
          <FaTrash className="w-4 h-4" />
        </button>
      </div>

      {/* Asset Header */}
      <h2 className={`text-2xl font-bold mb-4 border-b-[6px] ${pnl >= 0 ? 'border-green-400' : 'border-red-500' } rounded-t-2xl bg-[#1D1D1D] p-6 pb-7 border-gray-800 pb-2 uppercase tracking-wide`}>
        {asset}
      </h2>

      {/* Market Details Grid */}
      <div className="grid grid-cols-2 gap-4 mb-4 p-4">
        <div>
          <p className="text-xs text-gray-400 uppercase">Price</p>
          <p className="text-xl font-semibold mt-1">${price.toFixed(2)}</p>
        </div>
        <div>
          <p className="text-xs text-gray-400 uppercase">Leverage</p>
          <p className="text-xl font-semibold mt-1">{lev}×</p>
        </div>
        <div>
          <p className="text-xs text-gray-400 uppercase">Margin</p>
          <p className="text-xl font-semibold mt-1">${margin.toFixed(2)}</p>
        </div>
        <div>
          <p className="text-xs text-gray-400 uppercase">PnL</p>
          <p className={`text-xl font-semibold mt-1 ${pnl >= 0 ? 'text-green-400' : 'text-red-400'}`}>${pnl.toFixed(2)}</p>
        </div>
      </div>

      {/* Indicators */}
      <div className="flex flex-wrap gap-3 p-4">
        {indicators.map((ind, idx) => {
          const key = Object.keys(ind)[0];
          const label = indicatorLabels[key] || key;
          const colorClasses = indicatorColors[key] || 'bg-gray-800 text-gray-200';
          return (
            <span
              key={`${key}-${idx}`}
              className={`${colorClasses} text-indigo-200 text-xs font-medium px-3 py-1 rounded-full`}
            >
              {label}
            </span>
          );
        })}
      </div>
    </div>
  );
};

export default MarketCard;


