import type { WalletProvider } from "./provider";

interface PhantomEthereum {
    isPhantom?: boolean;
    request(args: { method: string; params?: unknown[] }): Promise<unknown>;
    on(event: string, handler: (...args: unknown[]) => void): void;
    removeListener(event: string, handler: (...args: unknown[]) => void): void;
    selectedAddress: string | null;
    isConnected(): boolean;
}

// Detect desktop Phantom
function getPhantomEthereum(): PhantomEthereum | null {
    if (typeof window === "undefined") return null;
    const w = window as Record<string, unknown>;
    const phantom = w.phantom as { ethereum?: PhantomEthereum } | undefined;
    const provider = phantom?.ethereum;
    if (provider?.isPhantom) return provider;
    return null;
}

// Detect mobile
const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);

export const phantomProvider: WalletProvider = {
    name: "Phantom",
    icon: "/phantom.svg",

    isAvailable(): boolean {
        return getPhantomEthereum() !== null || isMobile;
    },

    async connect(): Promise<string> {
        const provider = getPhantomEthereum();
        if (provider) {
            // Desktop flow
            const accounts = (await provider.request({
                method: "eth_requestAccounts",
            })) as string[];
            if (!accounts[0]) throw new Error("No account returned from Phantom");
            return accounts[0];
        }

        if (isMobile) {
            // External mobile browser: open dApp inside Phantom's in-app browser.
            // Once inside, window.phantom.ethereum is injected and the normal flow runs.
            // If Phantom is not installed the universal link falls back to phantom.app
            // where the user can download the app.
            const current = window.location.href;
            const ref = window.location.origin;
            window.location.href = `https://phantom.app/ul/browse/${encodeURIComponent(current)}?ref=${encodeURIComponent(ref)}`;
            return new Promise<string>(() => {}); // page is navigating away
        }

        throw new Error("Phantom wallet is not installed");
    },

    async signMessage(message: string): Promise<string> {
        const provider = getPhantomEthereum();
    if (!provider) throw new Error("Phantom wallet is not installed");
    const address = provider.selectedAddress;
    if (!address) throw new Error("Wallet not connected");

    const toHex = (str: string) =>
        "0x" +
        Array.from(new TextEncoder().encode(str))
            .map((b) => b.toString(16).padStart(2, "0"))
            .join("");

    const signature = (await provider.request({
        method: "personal_sign",
        params: [toHex(message), address],
    })) as string;

    return signature;
    },

    async signTypedData(payload: string): Promise<string> {
        const provider = getPhantomEthereum();
        if (!provider) throw new Error("Phantom wallet is not installed");
        const address = provider.selectedAddress;
        if (!address) throw new Error("Wallet not connected");

        const signature = (await provider.request({
            method: "eth_signTypedData_v4",
            params: [address, payload],
        })) as string;

        return signature;
    },
};
