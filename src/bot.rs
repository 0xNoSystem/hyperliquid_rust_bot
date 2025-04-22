

pub struct Bot{
    info_client: InfoClient,
    wallet: LocalWallet,
    public_key: String, markets: HashMap<String, Arc<Mutex<Market>>>,
}






impl Bot{

    pub async fn new(url: BaseUrl, wallet: LocalWallet, public_key: )
}
