import { createEIP1193Provider } from "./eip1193";
import type { WalletProvider } from "./provider";

// Rabby before MetaMask — Rabby sets isMetaMask=true for compat,
// resolveProvider guards against it, but UI order should reflect priority.
export const ALL_WALLETS: WalletProvider[] = [
    createEIP1193Provider({
        name: "Phantom",
        icon: "/phantom.svg",
        downloadUrl: "https://phantom.app/",
        windowKey: "phantom",
        nestedKey: "ethereum",
        flag: "isPhantom",
        mobileDeepLink: (url, ref) =>
            `https://phantom.app/ul/browse/${url}?ref=${ref}`,
    }),
    createEIP1193Provider({
        name: "Backpack",
        icon: "/backpack.png",
        downloadUrl: "https://backpack.app/",
        windowKey: "backpack",
        nestedKey: "ethereum",
        flag: "isBackpack",
        mobileDeepLink: (url, ref) =>
            `https://backpack.app/ul/browse/?url=${url}&ref=${ref}`,
    }),
    createEIP1193Provider({
        name: "Rabby",
        icon: "/rabby.svg",
        downloadUrl: "https://rabby.io/",
        windowKey: "ethereum",
        flag: "isRabby",
        mobileDeepLink: (_url, _ref) => "https://rabby.io/",
    }),
    createEIP1193Provider({
        name: "MetaMask",
        icon: "/metamask.svg",
        downloadUrl: "https://metamask.io/",
        windowKey: "ethereum",
        flag: "isMetaMask",
        mobileDeepLink: (url, _ref) => `https://metamask.app.link/dapp/${url}`,
    }),
    createEIP1193Provider({
        name: "Coinbase Wallet",
        icon: "/coinbase.svg",
        downloadUrl: "https://www.coinbase.com/wallet",
        windowKey: "coinbaseWalletExtension",
        flag: "isCoinbaseWallet",
        mobileDeepLink: (url, ref) =>
            `https://go.cb-w.com/dapp?cb_url=${url}&ref=${ref}`,
    }),
    createEIP1193Provider({
        name: "OKX Wallet",
        icon: "/okx.svg",
        downloadUrl: "https://www.okx.com/web3",
        windowKey: "okxwallet",
        flag: "isOKXWallet",
        mobileDeepLink: (url, _ref) => `okx://wallet/dapp/url?dappUrl=${url}`,
    }),
    createEIP1193Provider({
        name: "Trust Wallet",
        icon: "/trust.svg",
        downloadUrl: "https://trustwallet.com/",
        windowKey: "ethereum",
        flag: "isTrust",
        mobileDeepLink: (url, _ref) =>
            `https://link.trustwallet.com/open_url?coin_id=60&url=${url}`,
    }),
    createEIP1193Provider({
        name: "Rainbow",
        icon: "/rainbow.svg",
        downloadUrl: "https://rainbow.me/",
        windowKey: "ethereum",
        flag: "isRainbow",
        mobileDeepLink: (url, _ref) => `https://rnbwapp.com/wc?uri=${url}`,
    }),
    createEIP1193Provider({
        name: "Brave Wallet",
        icon: "/brave.svg",
        downloadUrl: "https://brave.com/wallet/",
        windowKey: "ethereum",
        flag: "isBraveWallet",
        mobileDeepLink: (_url, _ref) => "https://brave.com/wallet/",
    }),
];
