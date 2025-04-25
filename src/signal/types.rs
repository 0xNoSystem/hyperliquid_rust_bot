use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use arraydeque::{ArrayDeque, behavior::Wrapping};
use kwant::indicators::{Rsi, Atr, StochRsi, Price, Indicator, Ema, EmaCross, Sma, Adx};

use crate::trade_setup::TimeFrame;
use crate::helper::get_time_now;
use crate::MAX_HISTORY;


#[derive(Debug, Copy, Clone)]
pub struct ExecParams{
    pub margin: f32,
    pub lev: u32,
    pub tf: TimeFrame,
} 

impl ExecParams{
    pub fn new(margin: f32, lev:u32, tf: TimeFrame)-> Self{
       Self{
            margin,
            lev,
            tf,
        } 
    }
}

pub enum ExecParam{
    Margin(f32),
    Lev(u32),
    Tf(TimeFrame),
}


#[derive(Debug, Clone,Copy, PartialEq, Eq, Hash)]
pub enum IndicatorKind{
    Rsi{length: u32, stoch_length: u32, smoothing_length: Option<u32>},
    StochRsi{length: u32},
    Adx{periods: u32, di_length: u32},
    Atr(u32),
    Ema(u32),
    EmaCross{short:u32, long:u32},
    Sma(u32),
}

pub struct Handler{
    indicator: Box<dyn Indicator + Send>,
    pub is_active: bool,
}

impl Handler{

    pub fn new(indicator: IndicatorKind) -> Handler{
        Handler{
            indicator: match_kind(indicator),
            is_active: true,
        }
    }

    fn toggle(&mut self) -> bool{
        self.is_active = !self.is_active;
        self.is_active
    }

    pub fn update(&mut self,price: Price, after_close: bool){
        if !after_close{
            self.indicator.update_before_close(price);
        }else{
            self.indicator.update_after_close(price);
        }
    }
    pub fn get_value(&self) -> Option<f32>{
        self.indicator.get_last()
    }

    pub fn load<'a,I: IntoIterator<Item=&'a Price>>(&mut self, price_data: I){
        let data_vec: Vec<Price> = price_data.into_iter().cloned().collect();
        self.indicator.load(&data_vec);
    }

    pub fn reset(&mut self){
        self.indicator.reset();
    }
 
}

    


pub type IndexId = (IndicatorKind, TimeFrame);

fn match_kind(kind: IndicatorKind) -> Box<dyn Indicator + Send> {
    match kind {
        IndicatorKind::Rsi { length, stoch_length, smoothing_length } => {
            Box::new(Rsi::new(length, stoch_length, smoothing_length))
        }
        IndicatorKind::StochRsi { length} => {
            Box::new(Rsi::new(length, length, None))
        }
        IndicatorKind::Adx { periods, di_length } => {
            Box::new(Adx::new(periods, di_length))
        }
        IndicatorKind::Atr(period) => {
            Box::new(Atr::new(period))
        }
        IndicatorKind::Ema(period) => {
            Box::new(Ema::new(period))
        }
        IndicatorKind::EmaCross { short, long } => {
            Box::new(EmaCross::new(short, long))
        }
        IndicatorKind::Sma(period) => {
            Box::new(Sma::new(period))
        }
    }
}


type History = ArrayDeque<Price, MAX_HISTORY, Wrapping>;

pub struct Tracker{
    pub price_data: History,
    pub indicators: HashMap<IndicatorKind, Handler>,
    tf: TimeFrame,
    next_close: u64,
}



impl Tracker{
    pub fn new(tf: TimeFrame) -> Self{
        Tracker{
            price_data: ArrayDeque::new(),
            indicators: HashMap::new(),
            tf,
            next_close: Self::calc_next_close(tf),
        }
    }


    pub fn digest(&mut self, price: Price){
        let time = get_time_now(); 
       
        if time >= self.next_close{
            self.next_close = Self::calc_next_close(self.tf);
            self.price_data.push_back(price);
            self.update_indicators(price, true);
        }else{
            self.update_indicators(price, false);
        }
        
    }

    fn update_indicators(&mut self,price: Price, after_close: bool){

        for (kind, handler) in &mut self.indicators{
            handler.update(price, after_close);
        }
    }
    
    fn calc_next_close(tf: TimeFrame)-> u64 {
        let now = get_time_now();

        let tf_ms = tf.to_millis();
        ((now / tf_ms) + 1) * tf_ms
    }
    
    
    pub fn load<I: IntoIterator<Item=Price>>(&mut self, price_data: I){
        let buffer: Vec<Price> = price_data.into_iter().collect();
        for (_kind, handler) in &mut self.indicators{
            handler.load(&buffer);
        }

        self.price_data.extend(buffer);

    }


    pub fn add_indicator(&mut self, kind: IndicatorKind){
        let mut handler = Handler::new(kind);
        handler.load(&self.price_data);
        self.indicators.insert(kind, handler);
    }

    pub fn remove_indicator(&mut self, kind: IndicatorKind){
        self.indicators.remove(&kind);
    } 

    pub fn toggle_indicator(&mut self, kind: IndicatorKind){
        if let Some(handler) = self.indicators.get_mut(&kind){
            let _ = handler.toggle();
        }
    }
    
    pub fn reset(&mut self){
        self.price_data.clear();
        for (_kind, handler) in &mut self.indicators{
            handler.reset();
        }
    }
    
}




pub type TimeFrameData = HashMap<TimeFrame, Vec<Price>>;

#[derive(Copy, Clone, Debug,PartialEq)]
pub struct Entry{
    pub id: IndexId,
    pub edit: EditType
}

#[derive(Copy, Clone, Debug,PartialEq)]
pub enum EditType{
    Toggle,
    Add,
    Remove,
}










