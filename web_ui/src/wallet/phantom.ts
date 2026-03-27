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
            // Mobile flow: redirect to Phantom deeplink
            const appUrl = window.location.origin; // your DApp URL
            const deeplink = `https://phantom.app/ul/v1/connect?app_url=${encodeURIComponent(
                appUrl
            )}&redirect_link=${encodeURIComponent(appUrl)}`;

            // Redirect user to Phantom app
            window.location.href = deeplink;

            // Wait for user to come back and resolve address from query param
            return new Promise<string>((resolve, reject) => {
                const checkAddress = () => {
                    const params = new URLSearchParams(window.location.search);
                    const address = params.get("phantom_address");
                    if (address) {
                        resolve(address);
                    } else {
                        // Check again in a short interval
                        setTimeout(checkAddress, 500);
                    }
                };
                // Start checking
                setTimeout(checkAddress, 500);

                // Timeout fallback after 30 seconds
                setTimeout(() => reject(new Error("Failed to get Phantom address on mobile")), 30000);
            });
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
