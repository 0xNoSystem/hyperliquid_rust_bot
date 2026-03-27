import type { WalletProvider } from "./provider";

interface PhantomEthereum {
    isPhantom?: boolean;
    request(args: { method: string; params?: unknown[] }): Promise<unknown>;
    on(event: string, handler: (...args: unknown[]) => void): void;
    removeListener(event: string, handler: (...args: unknown[]) => void): void;
    selectedAddress: string | null;
    isConnected(): boolean;
}

function getProvider(): PhantomEthereum | null {
    if (typeof window === "undefined") return null;
    const w = window as Record<string, unknown>;
    const phantom = w.phantom as { ethereum?: PhantomEthereum } | undefined;
    if (phantom?.ethereum?.isPhantom) return phantom.ethereum;
    const eth = w.ethereum as PhantomEthereum | undefined;
    if (eth?.isPhantom) return eth;
    return null;
}

const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);

export const phantomProvider: WalletProvider = {
    name: "Phantom",
    icon: "/phantom.svg",
    downloadUrl: "https://phantom.app/",

    isAvailable(): boolean {
        return getProvider() !== null || isMobile;
    },

    async connect(): Promise<string> {
        const provider = getProvider();

        if (provider) {
            const accounts = (await provider.request({
                method: "eth_requestAccounts",
            })) as string[];
            if (!accounts[0])
                throw new Error("No account returned from Phantom");
            return accounts[0];
        }

        if (isMobile) {
            // External mobile browser: open dApp inside Phantom's in-app browser.
            // Requires HTTPS — eth_requestAccounts won't show a popup on HTTP.
            // Falls back to phantom.app (download page) if app is not installed.
            const url = window.location.href;
            const ref = window.location.origin;
            window.location.href = `https://phantom.app/ul/browse/${encodeURIComponent(url)}?ref=${encodeURIComponent(ref)}`;
            return new Promise<string>(() => {}); // navigating away
        }

        throw new Error("Phantom wallet not found");
    },

    async signMessage(message: string): Promise<string> {
        const provider = getProvider();
        if (!provider) throw new Error("Phantom not connected");
        const address = provider.selectedAddress;
        if (!address) throw new Error("Wallet not connected");

        const hex =
            "0x" +
            Array.from(new TextEncoder().encode(message))
                .map((b) => b.toString(16).padStart(2, "0"))
                .join("");

        return (await provider.request({
            method: "personal_sign",
            params: [hex, address],
        })) as string;
    },

    async signTypedData(payload: string): Promise<string> {
        const provider = getProvider();
        if (!provider) throw new Error("Phantom not connected");
        const address = provider.selectedAddress;
        if (!address) throw new Error("Wallet not connected");

        return (await provider.request({
            method: "eth_signTypedData_v4",
            params: [address, payload],
        })) as string;
    },
};
