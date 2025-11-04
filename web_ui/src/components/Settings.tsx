import React, { useState } from 'react';
import { AlertCircle, Link as LinkIcon } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { ArrowLeft } from "lucide-react";
import { useNavigate } from "react-router-dom";

export default function Settings() {
  const [privateKey, setPrivateKey] = useState('');
  const [agentKey, setAgentKey] = useState('');
  const [wallet, setWallet] = useState('');
  const [saved, setSaved] = useState(false);

  const navigate = useNavigate();

  const saveSettings = () => {
    localStorage.setItem('PRIVATE_KEY', privateKey);
    localStorage.setItem('AGENT_KEY', agentKey);
    localStorage.setItem('WALLET', wallet);
    setSaved(true);
    setTimeout(() => setSaved(false), 3000);
  };

  return (
    <div className="relative min-h-screen overflow-hidden bg-[#07090B] text-white px-6 py-10">
    <motion.button
  whileHover={{ scale: 1.05 }}
  whileTap={{ scale: 0.95 }}
  onClick={() => navigate(-1)}
  className="mb-6 flex items-center gap-3 text-white hover:text-cyan-300"
>
  <ArrowLeft className="w-6 h-6 sm:w-7 sm:h-7 md:w-8 md:h-8 lg:w-10 lg:h-10" />
  <span className="text-base sm:text-lg md:text-xl lg:text-2xl font-medium">Back</span>
</motion.button>
      {/* Background grid + glow */}
      <div className="my-14 rounded-md border border-white/10 bg-[#0B0E12]/80 p-5 text-sm text-white/70">
  <h3 className="text-white text-base font-semibold mb-3">How to generate your API keys and wallet address</h3>
  <ol className="list-decimal list-inside space-y-2">
    <li>
      Visit the 
      <a
        href="https://app.hyperliquid.xyz/API"
        target="_blank"
        rel="noopener noreferrer"
        className="ml-1 text-cyan-300 hover:underline"
      >
        Hyperliquid API page
      </a>.
    </li>
    <li>Connect your wallet (e.g. Backpack, MetaMask..) to authenticate.</li>
    <li>Enter API key name, you can name it whatever you like.</li>
    <li>Click “Generate”, than click "Authorize API Wallet"</li>
    <li>Select the number of days you'd like this key to be valid for</li>
    <li>Copy the generated <strong>Private Key</strong> (shown once only in a red box) and <strong>Agent Key(optional)</strong>. Store them securely.</li>
    <li>Copy your wallet address (same address you connected, the one you know). Paste it into the WALLET field below.</li>
    <li>Return to this page and fill in all three fields, then click “Save Settings”.</li>
  </ol>
  <p className="mt-4 text-red-400 italic">
    IMPORTANT: As mentioned on the HL website. These keys allow the bot to trade on your behalf. They do not allow fund transfers or withdrawals. You can always destroy the keys if you see fit (Click <strong>Remove</strong> where you generated the Key).
  </p>
</div>
      <div className="pointer-events-none absolute inset-0 opacity-[0.08] [background:radial-gradient(60%_60%_at_0%_0%,rgba(56,189,248,0.5),transparent_60%),radial-gradient(50%_50%_at_100%_0%,rgba(232,121,249,0.5),transparent_60%),radial-gradient(60%_60%_at_50%_100%,rgba(52,211,153,0.4),transparent_60%)]" />
      <div className="pointer-events-none absolute inset-0 opacity-[0.06] bg-[linear-gradient(transparent_23px,rgba(255,255,255,0.06)_24px),linear-gradient(90deg,transparent_23px,rgba(255,255,255,0.06)_24px)] bg-[size:26px_26px]" />

      <div className="relative mx-auto max-w-2xl rounded-md border border-white/10 bg-[#0B0E12]/80 p-6">
        <h2 className="text-xl font-semibold mb-4">API Key Settings</h2>
        <p className="text-sm text-white/60 mb-6">
          These keys only authorize trading through the bot. They <strong>cannot move funds</strong>. You can generate them from the
          <a
            href="https://app.hyperliquid.xyz/API"
            target="_blank"
            rel="noopener noreferrer"
            className="ml-1 inline-flex items-center gap-1 text-cyan-300 hover:underline"
          >
            Hyperliquid API Page <LinkIcon className="w-4 h-4" />
          </a>
        </p>

        <div className="space-y-4">
          <div>
            <label className="block text-sm text-white/70 mb-1">API PRIVATE KEY</label>
            <input
              type="password"
              className="w-full rounded-md bg-[#101214] border border-white/10 px-3 py-2 text-white"
              value={privateKey}
              onChange={e => setPrivateKey(e.target.value)}
              placeholder="Enter your API PRIVATE KEY"
            />
          </div>

          <div>
            <label className="block text-sm text-white/70 mb-1">AGENT KEY</label>
            <input
              type="text"
              className="w-full rounded-md bg-[#101214] border border-white/10 px-3 py-2 text-white"
              value={agentKey}
              onChange={e => setAgentKey(e.target.value)}
              placeholder="Enter your AGENT KEY (Optional)"
            />
          </div>

          <div>
            <label className="block text-sm text-white/70 mb-1">WALLET</label>
            <input
              type="text"
              className="w-full rounded-md bg-[#101214] border border-white/10 px-3 py-2 text-white"
              value={wallet}
              onChange={e => setWallet(e.target.value)}
              placeholder="Enter your WALLET address"
            />
          </div>

          <button
            onClick={saveSettings}
            className="mt-4 w-full rounded-md border border-cyan-400/40 bg-cyan-500/10 px-4 py-2 text-cyan-200 hover:bg-cyan-500/20"
          >
            Save Settings
          </button>
        </div>
      </div>

      {/* Save confirmation */}
      <AnimatePresence>
        {saved && (
          <motion.div
            initial={{ y: -16, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: -16, opacity: 0 }}
            className="fixed left-1/2 top-6 z-50 -translate-x-1/2"
          >
            <div className="flex items-center gap-2 rounded-md border border-green-500/40 bg-[#102A12] px-3 py-2 text-green-100 shadow">
              <AlertCircle className="h-4 w-4" />
              <span className="text-sm">Settings saved locally</span>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <style>{`@keyframes scan{0%{transform:translateX(0)}100%{transform:translateX(-25%)}}`}</style>
    </div>
  );
}

