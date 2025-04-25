use log::info;
use kwant::indicators::{Rsi, Atr, Price, Indicator, Ema, EmaCross, Sma, Adx};
use crate::trade_setup::{TimeFrame, PriceData,TradeParams,TradeCommand};
use crate::strategy::{Strategy, CustomStrategy, Style, Stance};
use crate::{MAX_HISTORY};
use tokio::sync::mpsc::UnboundedReceiver;
use flume::{TrySendError,Sender, bounded};
 
use tokio::time::{sleep, Duration};
use tokio::sync::mpsc::unbounded_channel;

#[derive(Debug)]
pub struct SignalEngine{
    engine_rv: UnboundedReceiver<EngineCommand>,
    trade_tx: Sender<TradeCommand>,
    trackers: HashMap<TimeFrame, Tracker>, 
    strategy: Strategy,
    exec_params: ExecParams,
}



impl SignalEngine{

    pub async fn new(
        config: Option<Vec<IndexId>>,
        trade_params: TradeParams,
        engine_rv: UnboundedReceiver<EngineCommand>,
        trade_tx: Sender<TradeCommand>, 
        margin: f32,
    ) -> Self{
        let mut trackers:HashMap<TimeFrame, Tracker> = HashMap::new();
        trackers.insert(trade_params.time_frame, Tracker::new(trade_params.time_frame);

        if let Some(list) = config{
            if !list.is_empty(){
                for id in list{
                    if let Some(tracker) = &mut trackers.get_mut(id.1){
                        tracker.add_indicator(id.0); 
                    }else{
                    let mut new_tracker = Tracker::new(id.1);
                    new_tracker.add_indicator(id.0); 
                    trackers.insert(id.1, new_tracker);
                    }
                }
            }};
            
        SignalEngine{
            engine_rv,
            trade_tx,
            trackers,
            exec_params: ExecParams::new(margin, trade_params.lev, trade_params.time_frame),
        }
    }

    pub fn reset(&mut self){
        for _tf, tracker in &mut self.trackers{
            tracker.reset();
        }
    } 
    
    
     pub fn add_indicator(&mut self, id: IndexId){
       if let Some(tracker) = &mut trackers.get_mut(id.1){
            tracker.add_indicator(id.0); 
        }else{
            let mut new_tracker = Tracker::new(id.1);
            new_tracker.add_indicator(id.0); 
            trackers.insert(id.1, new_tracker);
     }
    }



    pub fn get_active_indicators(&self) -> Vec<IndexId>{
        let mut active = Vec::new();
        for tf, tracker in &self.trackers{
            for kind, handler in &tracker.indicators{
                if handler.is_active{
                    active.push((*kind, *tf));
                }
            }
        }
        active
    }
    
    pub fn change_strategy(&mut self, strategy: Strategy){
        self.strategy = strategy;
        info!("Strategy changed to: {:?}", self.strategy);
    }
    pub fn get_strategy(&self) -> &Strategy{
        &self.strategy
    }

    pub fn load(&mut self,tf: TimeFrame, price_data: &Vec<Price>) {
      
        if let Some(tracker) = self.indicators.get_mut(tf){
            
        }
    }


    fn get_signal(&self, price: f32) -> Option<TradeCommand>{
        if !self.is_ready(){
            return None;
        }

        let ExecParams { margin, lev, tf } = self.exec_params;
        let tf = tf.to_secs();
        
        let size = ((margin * lev as f32) / price)*0.7;
        
        match self.strategy{
            Strategy::Custom(strat) =>{
            
        let duration = match strat.style{
            Style::Scalp => {tf * 4},
            Style::Swing => {tf * 10},
        };

        let rsi_range = strat.get_rsi_threshold();
        let stoch_range = strat.get_stoch_threshold();
        
        let up_trend = self.get_ema_cross_trend();
        let rsi = self.get_sma_rsi().unwrap_or(self.get_rsi().unwrap_or(50.0));
        let stoch = self.get_stoch_rsi().unwrap();
        let atr = self.get_atr_normalized(price).unwrap(); 
     
        match strat.style{
            Style::Scalp => {
                if rsi < rsi_range.low && stoch < stoch_range.low{
                    if atr < 0.03 {return None;}; //check if volatilty is high enough 
                    if strat.stance == Stance::Bear{
                        return None;
                    }else{
                        return Some(TradeCommand::ExecuteTrade{size, is_long: true,duration: duration});
                    }
                }else if rsi > rsi_range.high && stoch > stoch_range.high{
                    if strat.stance == Stance::Bull{
                        return None;
                    }else{
                        return Some(TradeCommand::ExecuteTrade{size, is_long: false,duration: duration});
                    }
                }else{
                    return None;
                }
            },
            
            Style::Swing =>{

                return None; 

            }, 

            _ => {
                return None;
            }
        }
    }

    }
}
}
impl SignalEngine{

    pub async fn start(&mut self){
        let mut time = 0;
        let mut init = false;
            
            while let Some(cmd) = self.engine_rv.recv().await{
           
            match cmd {

                EngineCommand::UpdatePrice(price_data) => {
                    let close_time = price_data.time;
                    let price = price_data.price.close;
                    println!("\n\nPRICE => {}$", price);
                    if !init{
                            self.update(price_data.price, false);
                            time = close_time;
                            init = true;
                            continue;
                        }


                    if time != close_time{
                            self.update(price_data.price, true); 
                            time = close_time;
                    }else{
                            self.update(price_data.price, false);
                    }

                        self.display_indicators(price);
                        
                        if let Some(trade_command) = self.get_signal(price){
                            let _ = self.trade_tx.try_send(trade_command);
                        }
            },

                    EngineCommand::UpdateStrategy(new_strat) =>{

                       self.change_strategy(new_strat);
                 },

                    EngineCommand::UpdateConfig(new_config) =>{
                        info!("NEW CONFIG: {:?}", new_config);
                        self.change_indicators_config(new_config);
                },
                
                    EngineCommand::UpdateExecParams(param)=>{
                        use ExecParam::*;
                        match param{
                            Margin(m)=>{
                                self.exec_params.margin = m;
                        },
                            Lev(l) => {
                                self.exec_params.lev = l;
                        },
                            Tf(t) => {
                                self.exec_params.tf = t;                                
                        },
                    }
                },
                EngineCommand::Reload{price_data, tf}=>{
                        self.exec_params.tf = tf;
                        self.reset();
                        self.load(&price_data);
                        time = 0;
                        init = false;
                        info!("RELOADED ENGINE WITH NEW TIME FRAME DATA");
                    },
                    EngineCommand::Stop =>{ 
                        return;
                    },
            }
        }
    }

    pub fn display_indicators(&mut self, price: f32){

                    if let Some(stoch_rsi) = self.get_stoch_rsi(){
                println!("🔵STOCH-K: {}", stoch_rsi);
            }
            
            if let Some(stoch_rsi) = self.get_stoch_signal(){
                println!("🟠STOCH-D: {}", stoch_rsi);
            }

            if let Some(rsi_value) = self.get_rsi(){
                println!("🟢RSI: {}", &rsi_value);
                    
            };

            if let Some(rsi_value) = self.get_sma_rsi(){
                println!("🟣SMA-RSI: {}",&rsi_value);
                
            }
            
            if let Some(atr_value) = self.get_atr_normalized(price){
                println!("🔴ATR (NORMALIZED) : {}", atr_value);
                println!("🔴ATR (RAW) : {}", self.get_atr().unwrap());
            }

            if let Some(adx_value) = self.get_adx(){
                println!("🟡ADX : {}", adx_value);
            }

            if let Some(ema) = self.get_ema(){
                println!("🟠EMA: {}", ema);
            }
            if let Some(sma) = self.get_sma(){
                println!("⚪SMA: {}", sma );
            }
            if let Some(trend) = self.get_ema_cross_trend(){
                println!("DOUBLE EMA uptrend: {}", trend );
            }
            if let Some(cross) = self.check_ema_cross(){
                let trend = if cross{"uptrend"} else{"downtrend"};
                println!("EMA cross: {}", trend);
            }
            
                
            
        }



        pub fn new_backtest(params: TradeParams, config: Option<IndicatorsConfig>, margin: f32) -> Self{
            
            let config: IndicatorsConfig = match config{
            Some(cfg) => cfg,
            None => IndicatorsConfig::default(),
        };

            let ema_cross = config.ema_cross_short_long_lenghts
            .map(|(short, long)| EmaCross::new(short, long));

        //channels won't be used in backtest, these are placeholders
        let (_tx, dummy_rv) = unbounded_channel::<EngineCommand>();
        let (dummy_tx, _rx) = bounded::<TradeCommand>(0);

        SignalEngine{
            engine_rv: dummy_rv,
            trade_tx: dummy_tx,
            indicators_config: config.clone(),
            rsi: Rsi::new(config.rsi_length, config.stoch_rsi_length ,config.rsi_smoothing),
            atr: Atr::new(config.atr_length),
            ema: Ema::new(config.ema_length),
            ema_cross: ema_cross,
            adx: Adx::new(config.adx_length, config.adx_length),
            sma: Sma::new(config.sma_length),
            strategy: params.strategy,
            price_data: Vec::new(),
            exec_params: ExecParams{margin, lev: params.lev, tf: params.time_frame},
        }           
        }

    
}






type IndexId = (IndicatorKind, TimeFrame);

pub enum EngineCommand{

    UpdatePrice(PriceData),
    UpdateStrategy(Strategy),
    UpdateConfig(IndicatorsConfig),
    UpdateExecParams(ExecParam),
    Reload{price_data: Vec<Price>, tf: TimeFrame},
    Stop,
}

