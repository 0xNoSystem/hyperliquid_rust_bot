import { useState } from "react";
import {
    Link as LinkIcon,
    ArrowLeft,
    AlertCircle,
    CheckCircle,
    ShieldCheck,
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { useNavigate } from "react-router-dom";
import { useAuth } from "../context/AuthContextStore";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { API_URL } from "../consts";

export default function Settings() {
    const [apiKey, setApiKey] = useState("");
    const [saving, setSaving] = useState(false);
    const [status, setStatus] = useState<{ ok: boolean; msg: string } | null>(
        null
    );

    const navigate = useNavigate();
    const { token } = useAuth();
    const { needsApiKey, setNeedsApiKey } = useWebSocketContext();

    const saveApiKey = async () => {
        if (!apiKey.trim()) return;
        setSaving(true);
        setStatus(null);
        try {
            const res = await fetch(`${API_URL}/api-key`, {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${token}`,
                },
                body: JSON.stringify({ api_key: apiKey }),
            });
            if (!res.ok) {
                const text = await res.text();
                throw new Error(text || `HTTP ${res.status}`);
            }
            setStatus({ ok: true, msg: "API key saved securely" });
            setNeedsApiKey(false);
            setApiKey("");
        } catch (e: unknown) {
            const msg = e instanceof Error ? e.message : "Failed to save";
            setStatus({ ok: false, msg });
        } finally {
            setSaving(false);
            setTimeout(() => setStatus(null), 4000);
        }
    };

    return (
        <div className="bg-app-bg/50 text-app-text relative min-h-screen overflow-hidden px-6 py-10 pb-50">
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

            {needsApiKey ? (
                <>
                    <div className="border-line-subtle bg-surface-pane text-app-text/70 my-14 rounded-md border p-5 text-sm">
                        <h3 className="text-app-text mb-3 text-base font-semibold">
                            How to generate your API key
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
                                Connect your wallet (the same one you logged in
                                with) to authenticate.
                            </li>
                            <li>
                                Enter an API key name — you can name it whatever
                                you like.
                            </li>
                            <li>
                                Click "Generate", then click "Authorize API
                                Wallet".
                            </li>
                            <li>
                                Select the number of days you'd like this key to
                                be valid for.
                            </li>
                            <li>
                                Copy the generated{" "}
                                <strong>Private API Key</strong> (shown once
                                only in a red box). Store it securely.
                            </li>
                            <li>
                                Return to this page, paste the key below, and
                                click "Save API Key".
                            </li>
                        </ol>
                        <p className="text-accent-danger-soft mt-4 italic">
                            This key allows the bot to trade on your behalf. It
                            does not allow fund transfers or withdrawals. You
                            can revoke it anytime from the Hyperliquid API page.
                        </p>
                    </div>

                    <div className="border-line-subtle bg-surface-pane relative mx-auto max-w-2xl rounded-md border p-6">
                        <h2 className="mb-4 text-xl font-semibold">API Key</h2>
                        <p className="text-app-text/60 mb-6 text-sm">
                            This key only authorizes trading through the bot. It{" "}
                            <strong>cannot move funds</strong>. Generate one
                            from the
                            <a
                                href="https://app.hyperliquid.xyz/API"
                                target="_blank"
                                rel="noopener noreferrer"
                                className="text-accent-info-link ml-1 inline-flex items-center gap-1 hover:underline"
                            >
                                Hyperliquid API Page{" "}
                                <LinkIcon className="h-4 w-4" />
                            </a>
                        </p>

                        <div className="space-y-4">
                            <div>
                                <label className="text-app-text/70 mb-1 block text-sm">
                                    PRIVATE API KEY
                                </label>
                                <input
                                    type="password"
                                    className="border-line-subtle bg-app-surface-4 text-app-text w-full rounded-md border px-3 py-2"
                                    value={apiKey}
                                    onChange={(e) => setApiKey(e.target.value)}
                                    placeholder="Enter your Hyperliquid API key"
                                />
                            </div>

                            <button
                                onClick={saveApiKey}
                                disabled={saving || !apiKey.trim()}
                                className="border-action-add-border bg-action-add-bg text-action-add-text hover:bg-action-add-hover mt-4 w-full rounded-md border px-4 py-2 disabled:opacity-50"
                            >
                                {saving ? "Saving..." : "Save API Key"}
                            </button>
                        </div>
                    </div>
                </>
            ) : (
                <div className="border-line-subtle bg-surface-pane relative mx-auto mt-14 max-w-2xl rounded-md border p-6">
                    <div className="flex items-center gap-3">
                        <ShieldCheck className="text-accent-success h-8 w-8" />
                        <div>
                            <h2 className="text-xl font-semibold">
                                API Key Connected
                            </h2>
                            <p className="text-app-text/60 text-sm">
                                Your Hyperliquid API key is configured and the
                                bot is running.
                            </p>
                        </div>
                    </div>
                </div>
            )}

            {/* Status toast */}
            <AnimatePresence>
                {status && (
                    <motion.div
                        initial={{ y: -16, opacity: 0 }}
                        animate={{ y: 0, opacity: 1 }}
                        exit={{ y: -16, opacity: 0 }}
                        className="fixed top-6 left-1/2 z-50 -translate-x-1/2"
                    >
                        <div
                            className={`flex items-center gap-2 rounded-md border px-3 py-2 shadow ${
                                status.ok
                                    ? "border-accent-success-strong/40 bg-surface-success text-success-faint"
                                    : "border-accent-danger-soft/40 bg-surface-pane text-accent-danger-soft"
                            }`}
                        >
                            {status.ok ? (
                                <CheckCircle className="h-4 w-4" />
                            ) : (
                                <AlertCircle className="h-4 w-4" />
                            )}
                            <span className="text-sm">{status.msg}</span>
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
}
