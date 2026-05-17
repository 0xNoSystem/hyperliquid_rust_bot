import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { X } from "lucide-react";
import {
    ALL_WALLETS,
    authenticateWallet,
    type WalletProvider,
} from "../wallet";
import { useAuth } from "../context/AuthContextStore";

const wallets: WalletProvider[] = ALL_WALLETS;
const FEATURED_WALLET_COUNT = 4;
const featuredWallets = wallets.slice(0, FEATURED_WALLET_COUNT);
const otherWallets = wallets.slice(FEATURED_WALLET_COUNT);

interface WalletConnectModalProps {
    open: boolean;
    onClose: () => void;
    onConnected?: () => void;
}

export default function WalletConnectModal({
    open,
    onClose,
    onConnected,
}: WalletConnectModalProps) {
    const [error, setError] = useState<string | null>(null);
    const [connecting, setConnecting] = useState(false);
    const [showOtherWallets, setShowOtherWallets] = useState(false);
    const { login } = useAuth();

    const handleConnect = async (wallet: WalletProvider) => {
        setError(null);
        setConnecting(true);
        try {
            if (!wallet.isAvailable()) {
                if (wallet.downloadUrl) {
                    window.open(wallet.downloadUrl, "_blank");
                }
                return;
            }
            const { token, address } = await authenticateWallet(wallet);
            login(token, address);
            onConnected?.();
            onClose();
        } catch (err) {
            const msg =
                err instanceof Error ? err.message : "Connection failed";
            setError(msg);
        } finally {
            setConnecting(false);
        }
    };

    return (
        <AnimatePresence>
            {open && (
                <div className="fixed inset-0 z-50">
                    <motion.div
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        className="bg-app-overlay absolute inset-0"
                        onClick={connecting ? undefined : onClose}
                    />
                    <div className="absolute inset-0 flex items-center justify-center p-4">
                        <motion.div
                            initial={{ opacity: 0, y: 18, scale: 0.98 }}
                            animate={{ opacity: 1, y: 0, scale: 1 }}
                            exit={{ opacity: 0, y: 12, scale: 0.98 }}
                            transition={{ duration: 0.18 }}
                            className="border-line-subtle bg-surface-pane text-app-text relative w-full max-w-sm rounded-xl border p-8 shadow-xl"
                        >
                            <button
                                type="button"
                                onClick={onClose}
                                disabled={connecting}
                                className="text-app-text/50 hover:text-app-text absolute top-3 right-3 rounded p-1 disabled:opacity-40"
                                aria-label="Close wallet connect"
                            >
                                <X className="h-4 w-4" />
                            </button>

                            <h2 className="mb-2 text-center text-lg font-semibold">
                                Connect Wallet
                            </h2>
                            <p className="text-app-text/50 mb-8 text-center text-sm">
                                Sign in with your wallet to manage markets and
                                approvals.
                            </p>

                            <div className="flex flex-col gap-3">
                                {featuredWallets.map((wallet) => (
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

                                {otherWallets.length > 0 && (
                                    <>
                                        <button
                                            type="button"
                                            onClick={() =>
                                                setShowOtherWallets(
                                                    (current) => !current
                                                )
                                            }
                                            disabled={connecting}
                                            className="hover:text-accent-brand text-app-text/80 w-full text-center text-sm transition disabled:opacity-50"
                                        >
                                            <span>
                                                {showOtherWallets
                                                    ? "▲ Hide "
                                                    : "▼ Show "}
                                                other wallets
                                            </span>
                                        </button>

                                        <AnimatePresence initial={false}>
                                            {showOtherWallets && (
                                                <motion.div
                                                    initial={{
                                                        opacity: 0,
                                                        height: 0,
                                                    }}
                                                    animate={{
                                                        opacity: 1,
                                                        height: "auto",
                                                    }}
                                                    exit={{
                                                        opacity: 0,
                                                        height: 0,
                                                    }}
                                                    className="flex flex-col gap-3 overflow-hidden"
                                                >
                                                    {otherWallets.map(
                                                        (wallet) => (
                                                            <button
                                                                key={
                                                                    wallet.name
                                                                }
                                                                onClick={() =>
                                                                    handleConnect(
                                                                        wallet
                                                                    )
                                                                }
                                                                disabled={
                                                                    connecting
                                                                }
                                                                className="border-line-subtle bg-app-surface-2 hover:bg-glow-10 flex w-full items-center gap-4 rounded-lg border px-4 py-3 transition disabled:opacity-50"
                                                            >
                                                                <img
                                                                    src={
                                                                        wallet.icon
                                                                    }
                                                                    alt={
                                                                        wallet.name
                                                                    }
                                                                    className="h-8 w-8 rounded-md"
                                                                />
                                                                <span className="text-sm font-medium">
                                                                    {connecting
                                                                        ? "Connecting..."
                                                                        : wallet.name}
                                                                </span>
                                                            </button>
                                                        )
                                                    )}
                                                </motion.div>
                                            )}
                                        </AnimatePresence>
                                    </>
                                )}
                            </div>

                            <AnimatePresence>
                                {error && (
                                    <motion.div
                                        initial={{ opacity: 0, y: 8 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        exit={{ opacity: 0, y: 8 }}
                                        className="border-accent-danger/40 bg-surface-danger-soft mt-5 rounded-md border px-4 py-2 text-center text-sm"
                                    >
                                        <span className="text-accent-danger-soft">
                                            {error}
                                        </span>
                                    </motion.div>
                                )}
                            </AnimatePresence>
                        </motion.div>
                    </div>
                </div>
            )}
        </AnimatePresence>
    );
}
