import { useState } from "react";
import { Link as LinkIcon, ArrowLeft, AlertCircle } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { useNavigate } from "react-router-dom";

export default function Settings() {
    const [privateKey, setPrivateKey] = useState("");
    const [agentKey, setAgentKey] = useState("");
    const [wallet, setWallet] = useState("");
    const [saved, setSaved] = useState(false);

    const navigate = useNavigate();

    const saveSettings = () => {
        localStorage.setItem("PRIVATE_KEY", privateKey);
        localStorage.setItem("AGENT_KEY", agentKey);
        localStorage.setItem("WALLET", wallet);
        setSaved(true);
        setTimeout(() => setSaved(false), 3000);
    };

    return (
        <div className="bg-app-bg text-app-text relative min-h-screen overflow-hidden px-6 py-10 pb-50">
            <motion.button
                whileHover={{ scale: 1.05 }}
                whileTap={{ scale: 0.95 }}
                onClick={() => navigate(-1)}
                className="text-app-text hover:text-accent-info-link mb-6 flex items-center gap-3"
            >
                <ArrowLeft className="h-6 w-6 sm:h-7 sm:w-7 md:h-8 md:w-8 lg:h-10 lg:w-10" />
                <span className="text-base font-medium sm:text-lg md:text-xl lg:text-2xl">
                    Back
                </span>
            </motion.button>
            {/* Background grid + glow */}
            <div className="border-line-subtle bg-surface-pane text-app-text/70 my-14 rounded-md border p-5 text-sm">
                <h3 className="text-app-text mb-3 text-base font-semibold">
                    How to generate your API keys and wallet address
                </h3>
                <ol className="list-inside list-decimal space-y-2">
                    <li>
                        Visit the
                        <a
                            href="https://app.hyperliquid.xyz/API"
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-accent-info-link ml-1 hover:underline"
                        >
                            Hyperliquid API page
                        </a>
                        .
                    </li>
                    <li>
                        Connect your wallet (e.g. Backpack, MetaMask..) to
                        authenticate.
                    </li>
                    <li>
                        Enter API key name, you can name it whatever you like.
                    </li>
                    <li>Click “Generate”, than click "Authorize API Wallet"</li>
                    <li>
                        Select the number of days you'd like this key to be
                        valid for
                    </li>
                    <li>
                        Copy the generated <strong>Private API Key</strong>{" "}
                        (shown once only in a red box) and{" "}
                        <strong>Agent Key(optional)</strong>. Store them
                        securely.
                    </li>
                    <li>
                        Copy your wallet address (same address you connected,
                        the one you know). Paste it into the WALLET field below.
                    </li>
                    <li>
                        Return to this page and fill in all three fields, then
                        click “Save Settings”.
                    </li>
                </ol>
                <p className="text-accent-danger-soft mt-4 italic">
                    IMPORTANT: As mentioned on the HL website. These keys allow
                    the bot to trade on your behalf. They do not allow fund
                    transfers or withdrawals. You can always destroy the keys if
                    you see fit (Click <strong>Remove</strong> where you
                    generated the Key).
                </p>
            </div>
            <div className="border-line-subtle bg-surface-pane relative mx-auto max-w-2xl rounded-md border p-6">
                <h2 className="mb-4 text-xl font-semibold">API Key Settings</h2>
                <p className="text-app-text/60 mb-6 text-sm">
                    These keys only authorize trading through the bot. They{" "}
                    <strong>cannot move funds</strong>. You can generate them
                    from the
                    <a
                        href="https://app.hyperliquid.xyz/API"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-accent-info-link ml-1 inline-flex items-center gap-1 hover:underline"
                    >
                        Hyperliquid API Page <LinkIcon className="h-4 w-4" />
                    </a>
                </p>

                <div className="space-y-4">
                    <div>
                        <label className="text-app-text/70 mb-1 block text-sm">
                            API KEY
                        </label>
                        <input
                            type="password"
                            className="border-line-subtle bg-app-surface-4 text-app-text w-full rounded-md border px-3 py-2"
                            value={privateKey}
                            onChange={(e) => setPrivateKey(e.target.value)}
                            placeholder="Enter your API KEY"
                        />
                    </div>

                    <div>
                        <label className="text-app-text/70 mb-1 block text-sm">
                            AGENT KEY
                        </label>
                        <input
                            type="text"
                            className="border-line-subtle bg-app-surface-4 text-app-text w-full rounded-md border px-3 py-2"
                            value={agentKey}
                            onChange={(e) => setAgentKey(e.target.value)}
                            placeholder="Enter your AGENT KEY (Optional)"
                        />
                    </div>

                    <div>
                        <label className="text-app-text/70 mb-1 block text-sm">
                            WALLET
                        </label>
                        <input
                            type="text"
                            className="border-line-subtle bg-app-surface-4 text-app-text w-full rounded-md border px-3 py-2"
                            value={wallet}
                            onChange={(e) => setWallet(e.target.value)}
                            placeholder="Enter your WALLET address"
                        />
                    </div>

                    <button
                        onClick={saveSettings}
                        className="border-action-add-border bg-action-add-bg text-action-add-text hover:bg-action-add-hover mt-4 w-full rounded-md border px-4 py-2"
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
                        className="fixed top-6 left-1/2 z-50 -translate-x-1/2"
                    >
                        <div className="border-accent-success-strong/40 bg-surface-success text-success-faint flex items-center gap-2 rounded-md border px-3 py-2 shadow">
                            <AlertCircle className="h-4 w-4" />
                            <span className="text-sm">
                                Settings saved locally
                            </span>
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
}
