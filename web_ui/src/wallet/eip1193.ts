import type { WalletProvider } from "./provider";

interface EIP1193Provider {
    request(args: { method: string; params?: unknown[] }): Promise<unknown>;
    selectedAddress?: string | null;
    [key: string]: unknown;
}

interface EIP1193WalletConfig {
    name: string;
    icon: string;
    downloadUrl?: string;
    /** Top-level key on window (e.g. "ethereum", "okxwallet", "phantom") */
    windowKey: string;
    /** Nested key under windowKey, if the provider lives at e.g. window.phantom.ethereum */
    nestedKey?: string;
    /** Boolean flag on the provider object identifying this wallet */
    flag: string;
    mobileDeepLink: (encodedUrl: string, encodedRef: string) => string;
}

function resolveProvider(cfg: EIP1193WalletConfig): EIP1193Provider | null {
    if (typeof window === "undefined") return null;
    const w = window as unknown as Record<string, unknown>;

    if (cfg.nestedKey) {
        const container = w[cfg.windowKey] as
            | Record<string, unknown>
            | undefined;
        const nested = container?.[cfg.nestedKey] as
            | EIP1193Provider
            | undefined;
        if (nested?.[cfg.flag]) return nested;
    }

    const top = w[cfg.windowKey] as EIP1193Provider | undefined;
    if (top?.[cfg.flag]) {
        // Rabby sets isMetaMask=true for compatibility — if we're looking for
        // MetaMask specifically and isRabby is also set, this is Rabby, not MetaMask.
        if (cfg.flag === "isMetaMask" && top["isRabby"]) return null;
        return top;
    }

    return null;
}

function encodeMessage(message: string): string {
    return (
        "0x" +
        Array.from(new TextEncoder().encode(message))
            .map((b) => b.toString(16).padStart(2, "0"))
            .join("")
    );
}

export function createEIP1193Provider(
    cfg: EIP1193WalletConfig
): WalletProvider {
    const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);
    let cachedAddress: string | null = null;

    return {
        name: cfg.name,
        icon: cfg.icon,
        downloadUrl: cfg.downloadUrl,

        isAvailable(): boolean {
            return resolveProvider(cfg) !== null || isMobile;
        },

        async connect(): Promise<string> {
            const provider = resolveProvider(cfg);
            if (provider) {
                const accounts = (await provider.request({
                    method: "eth_requestAccounts",
                })) as string[];
                if (!accounts[0])
                    throw new Error(`No account returned from ${cfg.name}`);
                cachedAddress = accounts[0];
                return accounts[0];
            }
            if (isMobile) {
                const url = encodeURIComponent(window.location.href);
                const ref = encodeURIComponent(window.location.origin);
                window.location.href = cfg.mobileDeepLink(url, ref);
                return new Promise<string>(() => {}); // navigating away
            }
            throw new Error(`${cfg.name} not found`);
        },

        async signMessage(message: string): Promise<string> {
            const provider = resolveProvider(cfg);
            if (!provider) throw new Error(`${cfg.name} not connected`);
            const address = provider.selectedAddress ?? cachedAddress;
            if (!address) throw new Error("Wallet not connected");
            return (await provider.request({
                method: "personal_sign",
                params: [encodeMessage(message), address],
            })) as string;
        },

        async signTypedData(payload: string): Promise<string> {
            const provider = resolveProvider(cfg);
            if (!provider) throw new Error(`${cfg.name} not connected`);
            const address = provider.selectedAddress ?? cachedAddress;
            if (!address) throw new Error("Wallet not connected");
            return (await provider.request({
                method: "eth_signTypedData_v4",
                params: [address, payload],
            })) as string;
        },
    };
}
