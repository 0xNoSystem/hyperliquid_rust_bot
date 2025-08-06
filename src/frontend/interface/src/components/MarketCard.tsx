import React from 'react';
import { FaPlay, FaPause, FaTrash } from 'react-icons/fa';
import type { IndicatorKind, MarketInfo, indicatorData, Decomposed  } from '../types';
import { indicatorLabels, indicatorColors, decompose, get_value, fromTimeFrame} from '../types';

interface MarketCardProps {
  market: MarketInfo;
  onTogglePause: (asset: string) => void;
  onRemove: (asset: string) => void;
}

const formatPrice = (price: number) => {
  const decimals = price < 1 ? 4 : 2;
  return price.toFixed(decimals);
};

const MarketCard: React.FC<MarketCardProps> = ({ market, onTogglePause, onRemove }) => {
  const { asset, price, lev, margin,params, pnl, is_paused, indicators} = market;
  const actionLabel = is_paused ? 'Resume' : 'Pause';
  const ActionIcon = is_paused ? FaPlay : FaPause;

  const { strategy } = params;
  const { risk, style, stance, followTrend } = strategy.custom;

  return (
    <div className="bg-[#1D1D1D] text-gray-100 rounded-2xl shadow-lg relative flex flex-col w-full max-w-lg mx-auto">
      {/* Action Buttons */}
      <div className="absolute top-4 right-4 flex space-x-2 mt-2 pr-6">
        <button
          onClick={() => onTogglePause(asset)}
          className="flex items-center justify-center p-3 bg-indigo-900 hover:bg-indigo-500 rounded-full focus:outline-none"
          title={`${actionLabel} trading`}
        >
          <ActionIcon className="w-4 h-4" />
        </button>
        <button
          onClick={() => onRemove(asset)}
          className="flex items-center justify-center p-3 bg-gray-600 hover:bg-red-500 hover:cursor-pointer rounded-full focus:outline-none"
          title="Remove market"
        >
          <FaTrash className="w-4 h-4" />
        </button>
      </div>

      {/* Asset Header */}
      <h2
        className={`text-2xl font-bold mb-4 border-b-[6px] pr-16 ${
          pnl >= 0 ? 'border-green-400' : 'border-red-500'
        } rounded-t-2xl bg-[#1D1D1D] p-6 pb-7 border-gray-800 uppercase tracking-wide`}
      >
        {asset}
      </h2>

      {/* Market Details */}
      <div className="grid grid-cols-2 gap-4 mb-4 p-4">
        <div>
          <p className="text-xs text-gray-400 uppercase">Price</p>
          <p className="text-xl font-semibold mt-1">${formatPrice(price)}</p>
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
          <p
            className={`text-xl font-semibold mt-1 ${
              pnl >= 0 ? 'text-green-400' : 'text-red-400'
            }`}
          >
            ${pnl.toFixed(2)}
          </p>
        </div>
      </div>

      {/* Indicators */}
     <div className="flex flex-wrap gap-3 px-4 pb-2">
  {indicators.map((data, i) => {
  const { kind, timeframe, value } = decompose(data);
  const kindKey = Object.keys(kind)[0];

  return (
    <div key={i} className="flex items-center gap-2">
      <span
        className={`${indicatorColors[kindKey]} px-3 py-1 rounded-full text-xs`}
        title={get_value(value)}
      >
        {indicatorLabels[kindKey] || kindKey} -- {fromTimeFrame(timeframe)}
      </span>
    </div>
  );
})}
</div>
{/* Strategy */}

      <div className="px-4 pb-4 pt-2 text-sm text-gray-300">
        <div className="mb-1">
          <span className="font-semibold text-white">Strategy:</span> {style} / {stance}
        </div>
        <div className="mb-1">
          <span className="font-semibold text-white">Risk:</span> {risk}
        </div>
        <div>
          <span className="font-semibold text-white">Trend Following:</span>{' '}
          {followTrend ? 'Yes' : 'No'}
        </div>
      </div>
    </div>
  );
};

export default MarketCard;

