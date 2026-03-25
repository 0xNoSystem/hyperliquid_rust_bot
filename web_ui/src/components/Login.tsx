import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { motion, AnimatePresence } from "framer-motion";
import { type WalletProvider, phantomProvider, authenticateWallet } from "../wallet";
import { useAuth } from "../context/AuthContextStore";
import RotatingCube from "./Cube";
import { BackgroundFX } from "./BackgroundFX";

const wallets: WalletProvider[] = [phantomProvider];

export default function Login() {
    const [error, setError] = useState<string | null>(null);
    const [connecting, setConnecting] = useState(false);
    const { login } = useAuth();
    const navigate = useNavigate();

    const handleConnect = async (wallet: WalletProvider) => {
        setError(null);
        setConnecting(true);
        try {
            if (!wallet.isAvailable()) {
                setError(`${wallet.name} is not installed`);
                return;
            }
            const { token, address } = await authenticateWallet(wallet);
            login(token, address);
            navigate("/", { replace: true });
        } catch (err) {
            const msg =
                err instanceof Error ? err.message : "Connection failed";
            setError(msg);
        } finally {
            setConnecting(false);
        }
    };

    return (
        <div className="bg-app-bg text-app-text relative flex min-h-screen flex-col items-center justify-center overflow-hidden">
            <BackgroundFX intensity={0.5} />

            <div className="relative z-10 flex flex-col items-center gap-10">
                {/* Logo */}
                <div className="flex items-center gap-4">
                    <div className="border-line-subtle bg-app-surface-2 grid place-items-center rounded-md border">
                        <RotatingCube foreground="#05DF72" />
                    </div>
                    <div className="leading-tight">
                        <h1 className="font-mono text-2xl tracking-[0.22em]">
                            KWANT
                        </h1>
                        <p className="text-app-text/50 text-[11px] uppercase tracking-widest">
                            Trading Terminal
                        </p>
                    </div>
                </div>

                {/* Card */}
                <motion.div
                    initial={{ opacity: 0, y: 16 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.4 }}
                    className="border-line-subtle bg-surface-pane w-full max-w-sm rounded-xl border p-8"
                >
                    <h2 className="mb-2 text-center text-lg font-semibold">
                        Connect Wallet
                    </h2>
                    <p className="text-app-text/50 mb-8 text-center text-sm">
                        Sign in with your wallet to continue
                    </p>

                    <div className="flex flex-col gap-3">
                        {wallets.map((wallet) => (
                            <button
                                key={wallet.name}
                                onClick={() => handleConnect(wallet)}
                                disabled={connecting}
                                className="border-line-subtle bg-app-surface-2 hover:bg-glow-10 flex w-full items-center gap-4 rounded-lg border px-4 py-3 transition disabled:opacity-50"
                            >
                                <img
                                    src={wallet.icon}
                                    alt={wallet.name}
                                    className="h-8 w-8 rounded-md"
                                />
                                <span className="text-sm font-medium">
                                    {connecting
                                        ? "Connecting..."
                                        : wallet.name}
                                </span>
                            </button>
                        ))}
                    </div>
                </motion.div>

                {/* Error */}
                <AnimatePresence>
                    {error && (
                        <motion.div
                            initial={{ opacity: 0, y: 8 }}
                            animate={{ opacity: 1, y: 0 }}
                            exit={{ opacity: 0, y: 8 }}
                            className="border-accent-danger/40 bg-surface-danger-soft max-w-sm rounded-md border px-4 py-2 text-center text-sm"
                        >
                            <span className="text-accent-danger-soft">
                                {error}
                            </span>
                        </motion.div>
                    )}
                </AnimatePresence>
            </div>
        </div>
    );
}
