import type { WalletProvider } from "./provider";

interface PhantomEthereum {
    isPhantom?: boolean;
    request(args: { method: string; params?: unknown[] }): Promise<unknown>;
    on(event: string, handler: (...args: unknown[]) => void): void;
    removeListener(event: string, handler: (...args: unknown[]) => void): void;
    selectedAddress: string | null;
    isConnected(): boolean;
}

function getPhantomEthereum(): PhantomEthereum | null {
    if (typeof window === "undefined") return null;
    const w = window as Record<string, unknown>;
    const phantom = w.phantom as { ethereum?: PhantomEthereum } | undefined;
    const provider = phantom?.ethereum;
    if (provider?.isPhantom) return provider;
    return null;
}

function toHex(str: string): string {
    return (
        "0x" +
        Array.from(new TextEncoder().encode(str))
            .map((b) => b.toString(16).padStart(2, "0"))
            .join("")
    );
}

export const phantomProvider: WalletProvider = {
    name: "Phantom",
    icon: "/phantom.svg",

    isAvailable(): boolean {
        return getPhantomEthereum() !== null;
    },

    async connect(): Promise<string> {
        const provider = getPhantomEthereum();
        if (!provider) {
            throw new Error("Phantom wallet is not installed");
        }
        const accounts = (await provider.request({
            method: "eth_requestAccounts",
        })) as string[];
        if (!accounts[0]) {
            throw new Error("No account returned from Phantom");
        }
        return accounts[0];
    },

    async signMessage(message: string): Promise<string> {
        const provider = getPhantomEthereum();
        if (!provider) {
            throw new Error("Phantom wallet is not installed");
        }
        const address = provider.selectedAddress;
        if (!address) {
            throw new Error("Wallet not connected");
        }
        const signature = (await provider.request({
            method: "personal_sign",
            params: [toHex(message), address],
        })) as string;
        return signature;
    },
};
