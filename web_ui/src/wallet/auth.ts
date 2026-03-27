import { API_URL } from "../consts";
import type { WalletProvider } from "./provider";

const SIGN_MESSAGE_PREFIX =
    "Sign this message to authenticate with Hyperliquid Terminal.\n\nNonce: ";

export interface AuthResult {
    token: string;
    address: string;
}

export async function authenticateWallet(
    wallet: WalletProvider
): Promise<AuthResult> {
    // 1. Connect wallet → get address
    const address = await wallet.connect();

    // 2. Request nonce from backend
    const nonceRes = await fetch(
        `${API_URL}/auth/nonce?address=${encodeURIComponent(address)}`
    );
    if (!nonceRes.ok) {
        throw new Error(`Failed to get nonce: ${nonceRes.status}`);
    }
    const { nonce } = (await nonceRes.json()) as { nonce: string };

    // 3. Sign the message (must match backend format exactly)
    const message = `${SIGN_MESSAGE_PREFIX}${nonce}`;
    const signature = await wallet.signMessage(message);

    // 4. Verify with backend → get JWT
    const verifyRes = await fetch(`${API_URL}/auth/verify`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ address, signature, nonce }),
    });
    if (!verifyRes.ok) {
        const status = verifyRes.status;
        if (status === 401) throw new Error("Signature verification failed");
        if (status === 410) throw new Error("Nonce expired, please try again");
        throw new Error(`Authentication failed: ${status}`);
    }
    const { token } = (await verifyRes.json()) as { token: string };

    return { token, address };
}
