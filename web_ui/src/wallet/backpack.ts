import type { WalletProvider } from "./provider";

interface BackpackEthereum {
    isBackpack?: boolean;
    request(args: { method: string; params?: unknown[] }): Promise<unknown>;
    on(event: string, handler: (...args: unknown[]) => void): void;
    removeListener(event: string, handler: (...args: unknown[]) => void): void;
    selectedAddress: string | null;
    isConnected(): boolean;
}

function getProvider(): BackpackEthereum | null {
    if (typeof window === "undefined") return null;
    const w = window as Record<string, unknown>;
    const backpack = w.backpack as { ethereum?: BackpackEthereum } | undefined;
    if (backpack?.ethereum?.isBackpack) return backpack.ethereum;
    const eth = w.ethereum as BackpackEthereum | undefined;
    if (eth?.isBackpack) return eth;
    return null;
}

const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);

let cachedAddress: string | null = null;

export const backpackProvider: WalletProvider = {
    name: "Backpack",
    icon: "/backpack.png",
    downloadUrl: "https://backpack.app/",

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
                throw new Error("No account returned from Backpack");
            cachedAddress = accounts[0];
            return accounts[0];
        }

        if (isMobile) {
            const url = encodeURIComponent(window.location.href);
            const ref = encodeURIComponent(window.location.origin);
            window.location.href = `https://backpack.app/ul/browse/?url=${url}&ref=${ref}`;
            return new Promise<string>(() => {}); // navigating away
        }

        throw new Error("Backpack wallet not found");
    },

    async signMessage(message: string): Promise<string> {
        const provider = getProvider();
        if (!provider) throw new Error("Backpack not connected");
        const address = provider.selectedAddress ?? cachedAddress;
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
        if (!provider) throw new Error("Backpack not connected");
        const address = provider.selectedAddress ?? cachedAddress;
        if (!address) throw new Error("Wallet not connected");

        return (await provider.request({
            method: "eth_signTypedData_v4",
            params: [address, payload],
        })) as string;
    },
};
