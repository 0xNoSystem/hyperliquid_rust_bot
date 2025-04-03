

pub struct Bot{
    info_client: InfoClient,
    wallet: LocalWallet,
    markets: HashMap<String, Arc<Mutex<Market>>>,
}


