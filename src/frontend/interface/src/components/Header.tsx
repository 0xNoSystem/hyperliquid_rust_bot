// =====================================================
// Header (Brutalist Theme)
// Sharp, monochrome with acid accent
// =====================================================

import React from 'react';
import { Github } from 'lucide-react';

const Header: React.FC = () => (
  <header className="flex items-center justify-between border-b border-white/20 bg-black px-8 py-4">
    <h1 className="flex items-center gap-3 font-mono text-3xl font-black text-white tracking-tight">
      KWANT
      <a
        href="https://app.hyperliquid.xyz"
        target="_blank"
        rel="noopener noreferrer"
        className="ml-1 flex items-center border border-lime-400 bg-lime-400/10 px-2 py-1 text-lime-300 hover:bg-lime-400/20"
      >
        <svg width="20" height="15" viewBox="0 0 21 16" xmlns="http://www.w3.org/2000/svg">
          <path
            d="M20.4523 7.53888C20.471 9.21764 20.1196 10.8218 19.4292 12.3544C18.4434 14.5368 16.0799 16.3213 13.9218 14.4218C12.1616 12.8736 11.8351 9.73059 9.19798 9.27049C5.7088 8.84769 5.62483 12.8923 3.34536 13.3492C0.804661 13.8653 -0.0380915 9.59381 -0.000774032 7.65391C0.0365434 5.71401 0.552769 2.98759 2.76072 2.98759C5.30142 2.98759 5.47245 6.83318 8.69731 6.62489C11.8911 6.40728 11.947 2.40624 14.0337 0.693288C15.8343 -0.786505 17.952 0.298469 19.0125 2.07982C19.9952 3.72749 20.4274 5.66116 20.4492 7.53888H20.4523Z"
            fill="lime"
          />
        </svg>
      </a>
      <span className="ml-2 text-base font-normal uppercase tracking-widest text-white/70">Trading Bot</span>
    </h1>

    <a
      href="https://github.com/0xNoSystem/hyperliquid_rust_bot"
      target="_blank"
      rel="noopener noreferrer"
      className="flex items-center gap-2 border border-white/30 bg-white/5 px-3 py-2 text-white hover:bg-white/10"
    >
      <Github className="h-5 w-5" />
      <span className="text-sm font-semibold">Repo</span>
    </a>
  </header>
);

export default Header;
