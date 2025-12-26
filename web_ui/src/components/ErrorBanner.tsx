import { AnimatePresence, motion } from "framer-motion";
import { AlertCircle, X } from "lucide-react";

type ErrorBannerProps = {
    message: string | null;
    onDismiss: () => void;
};

export function ErrorBanner({ message, onDismiss }: ErrorBannerProps) {
    return (
        <AnimatePresence>
            {message && (
                <motion.div
                    initial={{ y: -16, opacity: 0 }}
                    animate={{ y: 0, opacity: 1 }}
                    exit={{ y: -16, opacity: 0 }}
                    className="fixed top-6 left-1/2 z-50 -translate-x-1/2"
                >
                    <div className="flex items-center gap-2 rounded-md border border-accent-danger/40 bg-surface-danger px-3 py-2 text-danger-faint shadow">
                        <AlertCircle className="h-4 w-4" />
                        <span className="text-sm">{message}</span>
                        <button
                            onClick={onDismiss}
                            className="ml-2 rounded-md px-2 py-1 hover:bg-glow-10"
                            aria-label="Dismiss error"
                        >
                            <X className="h-4 w-4" />
                        </button>
                    </div>
                </motion.div>
            )}
        </AnimatePresence>
    );
}
