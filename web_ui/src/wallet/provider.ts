export interface WalletProvider {
    readonly name: string;
    readonly icon: string;
    readonly downloadUrl?: string;
    isAvailable(): boolean;
    connect(): Promise<string>;
    signMessage(message: string): Promise<string>;
    signTypedData(payload: string): Promise<string>;
}

export interface ConnectedWallet {
    address: string;
    provider: WalletProvider;
}
