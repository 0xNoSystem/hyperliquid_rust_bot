import { useState } from "react";
import {
    ArrowLeft,
    AlertCircle,
    CheckCircle,
    ShieldCheck,
    ExternalLink,
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { useNavigate } from "react-router-dom";
import { useAuth } from "../context/AuthContextStore";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { phantomProvider } from "../wallet";
import { API_URL } from "../consts";

function parseSignature(sigHex: string): {
    r: string;
    s: string;
    v: number;
} {
    const raw = sigHex.startsWith("0x") ? sigHex.slice(2) : sigHex;
    const r = "0x" + raw.slice(0, 64);
    const s = "0x" + raw.slice(64, 128);
    let v = parseInt(raw.slice(128, 130), 16);
    if (v < 27) v += 27;
    return { r, s, v };
}

export default function Settings() {
    const [agentName, setAgentName] = useState("");
    const [saving, setSaving] = useState(false);
    const [status, setStatus] = useState<{ ok: boolean; msg: string } | null>(
        null
    );

    const navigate = useNavigate();
    const { token } = useAuth();
    const { needsApiKey, setNeedsApiKey } = useWebSocketContext();

    const approveAgent = async () => {
        setSaving(true);
        setStatus(null);
        try {
            // 1. Prepare — get EIP-712 payload from backend
            const prepareRes = await fetch(`${API_URL}/agent/prepare`, {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${token}`,
                },
                body: JSON.stringify({
                    agent_name: agentName.trim() || null,
                }),
            });
            if (!prepareRes.ok) {
                const text = await prepareRes.text();
                throw new Error(text || `Prepare failed: ${prepareRes.status}`);
            }
            const { eip712Payload } = (await prepareRes.json()) as {
                eip712Payload: Record<string, unknown>;
            };

            // 2. Sign — ask Phantom to sign the EIP-712 typed data
            const sigHex = await phantomProvider.signTypedData(
                JSON.stringify(eip712Payload)
            );
            const signature = parseSignature(sigHex);

            // 3. Approve — send signature to backend, which POSTs to Hyperliquid
            const approveRes = await fetch(`${API_URL}/agent/approve`, {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${token}`,
                },
                body: JSON.stringify({ signature }),
            });
            if (!approveRes.ok) {
                const text = await approveRes.text();
                throw new Error(
                    text || `Approval failed: ${approveRes.status}`
                );
            }

            setNeedsApiKey(false);
            navigate("/", {
                replace: true,
                state: { agentApproved: true },
            });
        } catch (e: unknown) {
            const msg =
                e instanceof Error ? e.message : "Agent approval failed";
            setStatus({ ok: false, msg });
        } finally {
            setSaving(false);
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
                <div className="border-line-subtle bg-surface-pane relative mx-auto mt-14 max-w-2xl rounded-md border p-6">
                    <h2 className="mb-2 text-xl font-semibold">
                        Authorize Trading Agent
                    </h2>
                    <p className="text-app-text/60 mb-6 text-sm">
                        Sign a message with your wallet to authorize a
                        restricted trading agent. This agent can place orders on
                        your behalf but{" "}
                        <strong>cannot transfer or withdraw funds</strong>.
                    </p>

                    <div className="space-y-4">
                        <div>
                            <label className="text-app-text/70 mb-1 block text-sm">
                                AGENT NAME{" "}
                                <span className="text-app-text/40">
                                    (optional)
                                </span>
                            </label>
                            <input
                                type="text"
                                className="border-line-subtle bg-app-surface-4 text-app-text w-full rounded-md border px-3 py-2"
                                value={agentName}
                                onChange={(e) => setAgentName(e.target.value)}
                                placeholder="e.g. my-bot"
                                disabled={saving}
                            />
                            <p className="text-app-text/40 mt-1 text-xs">
                                Visible on Hyperliquid's API page for key
                                management
                            </p>
                        </div>

                        <button
                            onClick={approveAgent}
                            disabled={saving}
                            className="border-action-add-border bg-action-add-bg text-action-add-text hover:bg-action-add-hover mt-2 w-full rounded-md border px-4 py-2 disabled:opacity-50"
                        >
                            {saving
                                ? "Waiting for signature..."
                                : "Approve Agent"}
                        </button>
                    </div>

                    <p className="text-app-text/40 mt-5 text-center text-xs">
                        You can revoke this agent anytime from the{" "}
                        <a
                            href="https://app.hyperliquid.xyz/API"
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-accent-info-link inline-flex items-center gap-1 hover:underline"
                        >
                            Hyperliquid API page
                            <ExternalLink className="h-3 w-3" />
                        </a>
                    </p>
                </div>
            ) : (
                <div className="border-line-subtle bg-surface-pane relative mx-auto mt-14 max-w-2xl rounded-md border p-6">
                    <div className="flex items-center gap-3">
                        <ShieldCheck className="text-accent-success h-8 w-8" />
                        <div>
                            <h2 className="text-xl font-semibold">
                                Trading Agent Active
                            </h2>
                            <p className="text-app-text/60 text-sm">
                                Your trading agent is authorized and the bot is
                                running.
                            </p>
                        </div>
                    </div>
                    <p className="text-app-text/40 mt-4 text-xs">
                        Manage your API keys on the{" "}
                        <a
                            href="https://app.hyperliquid.xyz/API"
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-accent-info-link inline-flex items-center gap-1 hover:underline"
                        >
                            Hyperliquid API page
                            <ExternalLink className="h-3 w-3" />
                        </a>
                    </p>
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
