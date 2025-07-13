import { useState } from 'react';
import MarketCard from './MarketCard';
import type { MarketInfo } from '../types';

export default function MarketsPage() {
  const [markets, setMarkets] = useState<MarketInfo[]>(marks);
  const [marketToRemove, setMarketToRemove] = useState<string | null>(null);

  const remove_market = async (asset: string) => {
    await fetch('http://localhost:8090/market', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ removeMarket: asset.toUpperCase() }),
    });
  };

  const toggle_market = async (asset: string) => {
    await fetch('http://localhost:8090/market', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ toggleMarket: asset.toUpperCase() }),
    });
  };

  const handleTogglePause = (asset: string) => {
    setMarkets(prev =>
      prev.map(m => {
        if (m.asset === asset) {
          toggle_market(asset);
          return { ...m, is_paused: !m.is_paused };
        } else {
          return m;
        }
      })
    );
  };

  const onRemove = (asset: string) => {
    remove_market(asset);
    setMarkets(prev => prev.filter(m => m.asset !== asset));
  };

  return (
    <div className="p-8 space-y-4 bg-gray-600 bg-opacity-2">
      {markets.length === 0 && (
        <p className="text-gray-400">Add markets below</p>
      )}
      {markets.map((market) => (
        <MarketCard
          key={market.asset}
          market={market}
          onTogglePause={handleTogglePause}
          onRemove={(asset) => setMarketToRemove(asset)}
        />
      ))}

{marketToRemove && (
  <div className="fixed inset-0 flex items-center justify-center bg-indigo-600 bg-opacity-50">
    <div className="bg-[#1D1D1D] text-white p-6 rounded">
      <p>
        Are you sure you want to remove <strong>{marketToRemove.toUpperCase()}</strong>? 
        Any active position will be closed.
      </p>
      <div className="mt-4 flex gap-2">
        <button
          className="bg-red-600 text-white px-4 py-2 rounded hover:cursor-pointer"
          onClick={() => {
            onRemove(marketToRemove);
            setMarketToRemove(null);
          }}
        >
          Yes, Remove
        </button>
        <button
          className="bg-gray-300 px-4 py-2 rounded hover:cursor-pointer"
          onClick={() => setMarketToRemove(null)}
        >
          Cancel
        </button>
      </div>
    </div>
  </div>
)}
    </div>
  );
}




const sampleMarket: MarketInfo = {
  asset: 'BTC',
  price: 27345.12,
  lev: 10,
  margin: 500,
  pnl: -25.5,
  is_paused: false,
  indicators: [
    { rsi: 14 },
    { adx: { periods: 14, diLength: 14 } },
    { atr: 14 },
  ] as IndicatorKind[],
};

const solMarket: MarketInfo = {
  asset: 'SOL',
  price: 298.12,
  lev: 10,
  margin: 4959.3,
  pnl: 1233,
  is_paused: true,
  indicators: [
    { ema: 14 },
    { adx: { periods: 14, diLength: 14 } },
    { sma: 14 },
  ] as IndicatorKind[],
};


const fartMarket: MarketInfo = {
  asset: 'FARTCOIN',
  price: 11.12,
  lev: 3,
  margin: 238.98,
  pnl: 33.3,
  is_paused: false,
  indicators: [
    { emaCross: {short: 6, long: 14 }},
    { stochRsi: {periods: 10} },
    { sma: 14 },
    {rsi: 12},
    { adx: { periods: 14, diLength: 14 } },
  ] as IndicatorKind[],
};

const marks: MarketInfo[] = [fartMarket, solMarket, sampleMarket];
