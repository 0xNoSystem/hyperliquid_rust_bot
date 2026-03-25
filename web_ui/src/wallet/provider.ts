export interface WalletProvider {
    readonly name: string;
    readonly icon: string;
    isAvailable(): boolean;
    connect(): Promise<string>;
    signMessage(message: string): Promise<string>;
}

export interface ConnectedWallet {
    address: string;
    provider: WalletProvider;
}
