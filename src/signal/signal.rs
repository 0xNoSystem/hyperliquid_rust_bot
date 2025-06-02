use std::collections::HashMap;
use log::info;

use kwant::indicators::{Rsi, Atr, Price, Indicator, Ema, EmaCross, Sma, Adx, Value};

use crate::trade_setup::{TimeFrame,TradeParams, TradeCommand};
use crate::strategy::{Strategy, CustomStrategy, Style, Stance};
use crate::MAX_HISTORY;

use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::time::{sleep, Duration};
use flume::{Sender, TrySendError, bounded};

use super::types::{
    Tracker,
    Handler,
    IndexId,
    IndicatorKind,
    ExecParam,
    ExecParams,
    TimeFrameData,
    EditType,
    Entry,
};


pub struct SignalEngine{
    engine_rv: UnboundedReceiver<EngineCommand>,
    trade_tx: Sender<TradeCommand>,
    trackers: HashMap<TimeFrame, Box<Tracker>>, 
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
        let mut trackers:HashMap<TimeFrame, Box<Tracker>> = HashMap::new();
        trackers.insert(trade_params.time_frame, Box::new(Tracker::new(trade_params.time_frame)));

        if let Some(list) = config{
            if !list.is_empty(){
                for id in list{
                    if let Some(tracker) = &mut trackers.get_mut(&id.1){
                        tracker.add_indicator(id.0, false); 
                    }else{
                    let mut new_tracker = Tracker::new(id.1);
                    new_tracker.add_indicator(id.0, false); 
                    trackers.insert(id.1, Box::new(new_tracker));
                    }
                }
            }};
            
        SignalEngine{
            engine_rv,
            trade_tx,
            trackers,
            strategy: trade_params.strategy,
            exec_params: ExecParams::new(margin, trade_params.lev, trade_params.time_frame),
        }
    }

    pub fn reset(&mut self){
        for (_tf, tracker) in &mut self.trackers{
            tracker.reset();
        }
    } 
    
    
    pub fn add_indicator(&mut self, id: IndexId){
       if let Some(tracker) = &mut self.trackers.get_mut(&id.1){
            tracker.add_indicator(id.0, true); 
        }else{
            let mut new_tracker = Tracker::new(id.1);
            new_tracker.add_indicator(id.0, false); 
            self.trackers.insert(id.1, Box::new(new_tracker));
     }
    }

    pub fn remove_indicator(&mut self, id: IndexId){
        if let Some(tracker) = &mut self.trackers.get_mut(&id.1){
            tracker.remove_indicator(id.0); 
        }
    }

    pub fn toggle_indicator(&mut self, id: IndexId){
        if let Some(tracker) = &mut self.trackers.get_mut(&id.1){
            tracker.toggle_indicator(id.0); 
    }
}

    pub fn get_active_indicators(&self) -> Vec<IndexId>{
        let mut active = Vec::new();
        for (tf, tracker) in &self.trackers{
            for (kind, handler) in &tracker.indicators{
                if handler.is_active{
                    active.push((*kind, *tf));
                }
            }
        }
        active
    }

    pub fn get_active_values(&self) -> Vec<Value>{
        let mut values = Vec::new();
            for (tf, tracker) in &self.trackers{
                values.extend(tracker.get_active_values()); 
            }
        values
        }

    pub fn display_values(&self){
        for (tf, tracker) in &self.trackers{
            for (kind, handler) in &tracker.indicators{
                if handler.is_active{
                    println!("\nKind: {:?} TF: {}\nValue: {:?}\n", kind, tf.as_str(), handler.get_value());
                }
            }
        }
    }
    
    pub fn change_strategy(&mut self, strategy: Strategy){
        self.strategy = strategy;
        info!("Strategy changed to: {:?}", self.strategy);
    }

    pub fn get_strategy(&self) -> &Strategy{
        &self.strategy
    }

    pub fn load<I:IntoIterator<Item=Price>>(&mut self,tf: TimeFrame, price_data: I) {
        if let Some(tracker) = self.trackers.get_mut(&tf){
            tracker.load(price_data)    
        }
    }


    fn get_signal(&self, price: f32) -> Option<TradeCommand>{
        use Value::*;
        let risk_pct = 0.02;
        let values = self.get_active_values();
       
        match self.strategy{
            Strategy::Custom(brr) => brr.generate_signal(values, price)
        }
    }

}

impl SignalEngine{

    pub async fn start(&mut self){
            
            while let Some(cmd) = self.engine_rv.recv().await{
           
            match cmd {

                EngineCommand::UpdatePrice(price) => {
                    for (_tf, tracker) in &mut self.trackers{
                            tracker.digest(price);
                        }
                     self.display_indicators(price.close);
                    if let Some(trade) = self.get_signal(price.close){
                        self.trade_tx.try_send(trade);
                    }
                }, 

                EngineCommand::UpdateStrategy(new_strat) =>{
                    self.change_strategy(new_strat);
                 },

                
                EngineCommand::EditIndicators{indicators, price_data} =>{
                    info!("Received Indicator Edit Vec of length : {}", indicators.len()); 
                    

                    for entry in indicators{
                        match entry.edit{
                            EditType::Add => {
                                self.add_indicator(entry.id);
                                println!("ADDED FINISH");
                            },
                            EditType::Remove => {self.remove_indicator(entry.id);},
                            EditType::Toggle => {self.toggle_indicator(entry.id)},
                        }
                    }
                    if let Some(data) = price_data{
                        for (tf, prices) in data{
                            self.load(tf, prices);
                        }
                    }
                   
                }
                
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

                EngineCommand::Stop =>{ 
                    return;
                },
            }
        }
    }

    pub fn display_indicators(&mut self, price: f32){
            println!("\nPrice => {}\n", price);
            //let vec = self.get_active_indicators();      
            self.display_values(); 
            //Update 
        }



        pub fn new_backtest(trade_params: TradeParams, config: Option<Vec<IndexId>>, margin: f32) -> Self{
            let mut trackers:HashMap<TimeFrame, Box<Tracker>> = HashMap::new();
            trackers.insert(trade_params.time_frame, Box::new(Tracker::new(trade_params.time_frame)));

            if let Some(list) = config{
                if !list.is_empty(){
                    for id in list{
                        if let Some(tracker) = &mut trackers.get_mut(&id.1){ 
                            tracker.add_indicator(id.0, false); 
                        }else{
                            let mut new_tracker = Tracker::new(id.1);
                            new_tracker.add_indicator(id.0, false); 
                            trackers.insert(id.1, Box::new(new_tracker));
                    }
                }
            }}
   

        //channels won't be used in backtest, these are placeholders
        let (_tx, dummy_rv) = unbounded_channel::<EngineCommand>();
        let (dummy_tx, _rx) = bounded::<TradeCommand>(0);

        SignalEngine{
            engine_rv: dummy_rv,
            trade_tx: dummy_tx,
            trackers,
            strategy: trade_params.strategy,
            exec_params: ExecParams{margin, lev: trade_params.lev, tf: trade_params.time_frame},
        }           
    }
}




pub enum EngineCommand{

    UpdatePrice(Price),
    UpdateStrategy(Strategy),
    EditIndicators{indicators: Vec<Entry>,price_data: Option<TimeFrameData>},
    UpdateExecParams(ExecParam),
    Stop,
}









