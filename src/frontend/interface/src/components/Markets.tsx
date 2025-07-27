import React, { useState, useEffect, useRef } from 'react';
import MarketCard from './MarketCard';
import { AddMarket } from './AddMarket';
import type { MarketInfo, IndicatorKind, Message, assetPrice, TradeInfo, assetMargin, indicatorData, editMarketInfo } from '../types';

const sampleMarket: MarketInfo = {
  asset: 'BTC', price: 27345.12, lev: 10, margin: 500, pnl: -25.5, is_paused: false,
  indicators: [
    { rsi: 14 },
    { adx: { periods: 14, diLength: 14 } },
    { atr: 14 },
  ] as IndicatorKind[],
};
const solMarket: MarketInfo = {
  asset: 'SOL', price: 298.12, lev: 10, margin: 4959.3, pnl: 1233, is_paused: false,
  indicators: [
    { ema: 14 },
    { adx: { periods: 14, diLength: 14 } },
    { sma: 14 },
  ] as IndicatorKind[],
};
const fartMarket: MarketInfo = {
  asset: 'FARTCOIN', price: 11.12, lev: 3, margin: 238.98, pnl: 33.3, is_paused: false,
  indicators: [
    { emaCross: { short: 6, long: 14 } },
    { stochRsi: { periods: 10, kSmoothing: undefined, dSmoothing: undefined } },
    { sma: 14 }, { rsi: 12 }, { adx: { periods: 14, diLength: 14 } },
  ] as IndicatorKind[],
};
const initialMarkets: MarketInfo[] = [sampleMarket, solMarket, fartMarket];

export default function MarketsPage() {

  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const errorTimeoutRef = useRef<NodeJS.Timeout | null>(null);

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
      ws.onopen = () => console.log('WebSocket connected');
      ws.onmessage = (event: MessageEvent) => {
        const payload = JSON.parse(event.data) as Message;
        if ('confirmMarket' in payload) {
          setMarkets(prev => [...prev, payload.confirmMarket]);
        } else if ('updatePrice' in payload) {
          const [asset, price] = payload.updatePrice as assetPrice;
          setMarkets(prev => prev.map(m => m.asset === asset ? { ...m, price } : m));
        } else if ('newTradeInfo' in payload) {
          console.log('Trade info', (payload.newTradeInfo as TradeInfo));
        } else if ('updateTotalMargin' in payload) {
          setTotalMargin(payload.updateTotalMargin)
        } else if ('updateMarketMargin' in payload) {
          const [asset, margin] = payload.updateMarketMargin as assetMargin;
          setMarkets(prev => prev.map(m => m.asset === asset ? { ...m, margin } : m));
        } else if ('updateIndicatorValues' in payload) {
          const { asset, data } = payload.updateIndicatorValues as {asset:string, data:indicatorData[]};
          console.log('Indicator update', asset, data);
        }else if ('userError' in payload) {
            setErrorMsg(payload.userError);
            if (errorTimeoutRef.current) clearTimeout(errorTimeoutRef.current);
                errorTimeoutRef.current = setTimeout(() => setErrorMsg(null), 5000);        }

      };
      ws.onerror = err => console.error('WebSocket error', err);
      ws.onclose = () => {
        console.warn('WebSocket closed, reconnecting in 1s');
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
      method: 'POST', headers: {'Content-Type':'application/json'},
      body: JSON.stringify({ removeMarket: asset.toUpperCase() }),
    });
  };
  const toggle_market = async (asset: string) => {
    await fetch('http://localhost:8090/command', {
      method: 'POST', headers: {'Content-Type':'application/json'},
      body: JSON.stringify({ toggleMarket: asset.toUpperCase() }),
    });
  };
  const handleConfirmToggle = (asset: string, isPaused: boolean) => {
    if (isPaused) {
      toggle_market(asset);
      setMarkets(prev => prev.map(m => m.asset===asset?{...m,is_paused:false}:m));
    } else setMarketToToggle(asset);
  };
  const handleTogglePause = (asset: string) => {
    toggle_market(asset);
    setMarkets(prev => prev.map(m => m.asset===asset?{...m,is_paused:true}:m));
    setMarketToToggle(null);
  };
  const handleRemove = (asset: string) => {
    remove_market(asset);
    setMarkets(prev => prev.filter(m => m.asset !== asset));
    setMarketToRemove(null);
  };

  const closeAll = async () => { 
        await fetch('http://localhost:8090/command', {
      method: 'POST', headers: {'Content-Type':'application/json'},
      body: JSON.stringify({closeAll: null}),
    });
  };

  
  const pauseAll = async () => { 
        await fetch('http://localhost:8090/command', {
      method: 'POST', headers: {'Content-Type':'application/json'},
            body: JSON.stringify({pauseAll: null}),
    });
  };

  return (
    <div className="p-12  bg-[#333536] bg-opacity-20 h-screen bg-special pb-16">
{errorMsg && (
  <div className="fixed top-40 left-1/2 transform -translate-x-1/2 z-200 bg-red-500 text-white rounded shadow flex items-center group">
    <span className="ml-1 py-3 px-8">{errorMsg}</span>
    <button
      onClick={() => setErrorMsg(null)}
      className="text-white opacity-0 mr-2 cursor-pointer group-hover:opacity-100 transition-opacity duration-200"
    >
      ✕
    </button>
  </div>
)}
    <div className="flex justify-between">
    <div className="flex flex-col items-start ml-2 mb-12 space-y-6">
        <div className="text-white text-base font-semibold">
            Available Margin :  
            <span className="pl-2 tracking-wider font-normal">
                {totalMargin.toFixed(2)}
            </span>
        </div>

            {markets.length !== 0 && <button className="bg-orange-300 border border-white text-black font-semibold px-4 py-2 rounded hover:bg-orange-200 cursor-pointer" onClick={()=>setShowAdd(true)}>
            Add Market
        </button>}
    </div>      
    <div className="mt-12">
         <button
            className="bg-gray-500 text-black font-semibold px-4 py-2 rounded hover:bg-red-800 cursor-pointer mx-3"
            onClick={() => {
                closeAll();
                setMarkets([]);
            }}
        >
            CLOSE ALL
        </button>
         <button
            className="bg-orange-200 text-black font-semibold px-4 py-2 rounded hover:bg-green-200 cursor-pointer"
            onClick={
            () => {
                pauseAll();
                markets.map(m => m.is_paused = true);
            }}
        >
            PAUSE ALL
        </button>

    </div>
    </div>

    {showAdd && <AddMarket onClose={()=>setShowAdd(false)} totalMargin={totalMargin}/>}      
      {markets.length===0 && 
          <div className="flex flex-col items-center justify-center">
            <p className="text-gray-400 text-center text-xl p-4 mb-4">Add markets below</p>
            <button className="bg-orange-300 border border-white text-black font-semibold px-4 py-2 rounded hover:bg-orange-200 cursor-pointer" onClick={()=>setShowAdd(true)}>
                Add Market
            </button>

          </div>}
      <div className="flex flex-wrap gap-12 justify-start items-center ml-20">
        {markets.map(m=>(
          <div key={m.asset} className="inline-block" style={{zoom:0.90,width:'16rem'}}>
            <MarketCard market={m} onTogglePause={()=>handleConfirmToggle(m.asset,m.is_paused)} onRemove={()=>setMarketToRemove(m.asset)}/>
          </div>
        ))}
      </div>
      {marketToRemove && (
        <div className="fixed inset-0 z-50 flex items-center justify-center backdrop-blur-sm">
          <div className="bg-[#1D1D1D] text-white p-6 rounded">
          <p>Remove <strong>{marketToRemove}</strong>?</p>
          <p className="text-red-200 font-semibold" >This will close any ongoing trade initiated by the Bot</p>
            <div className="mt-4 flex gap-2">
              <button className="bg-red-600 text-white px-4 py-2 rounded cursor-pointer hover:bg-red-900" onClick={()=>handleRemove(marketToRemove)}>Yes</button>
              <button className="bg-gray-300 px-4 py-2 rounded" onClick={()=>setMarketToRemove(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}
      {marketToToggle && (
        <div className="fixed inset-0 flex items-center justify-center backdrop-blur-sm">
          <div className="bg-[#1D1D1D] text-white p-6 rounded"><p>Pause <strong>{marketToToggle}</strong>?</p>
                <p className="text-red-200 font-semibold" >This will close any ongoing trade initiated by the Bot</p>
            <div className="mt-4 flex gap-2">
              <button className="bg-yellow-600 text-white px-4 py-2 rounded" onClick={()=>handleTogglePause(marketToToggle)}>Yes</button>
              <button className="bg-gray-300 px-4 py-2 rounded" onClick={()=>setMarketToToggle(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

