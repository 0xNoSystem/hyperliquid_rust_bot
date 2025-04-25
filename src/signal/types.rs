use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Copy, Clone)]
struct ExecParams{
    margin: f32,
    lev: u32,
    tf: TimeFrame,
} 

impl ExecParams{
    fn new(margin: f32, lev:u32, tf: TimeFrame)-> Self{
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
    StochRsi{length: u32,k_smoothing: u32, d_smoothing: u32},
    Adx{periods: u32, di_length: u32},
    Atr(u32),
    Ema(u32),
    EmaCross{short:u32, long:u32},
    Sma(u32),
}

#[derive(Clone, Debug)]
pub struct Handler{
    indicator: Box<dyn Indicator>,
    pub is_active: bool,
}

impl Handler{

    pub fn new(indicator: IndicatorKind,tf: TimeFrame) -> Handler{
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
        self.indicator.get_last();
    }

    pub fn load(&mut self, price_data: Vec<Price>){
        self.indicator.load(&price_data);
    }

    pub fn reset(&mut self){
        self.indicator.reset();
    }
 
}

    


pub type IndexId = (IndicatorKind, TimeFrame);

fn match_kind(kind: IndicatorKind) -> Box<dyn Indicator> {
    match kind {
        IndicatorKind::Rsi { length, stoch_length, smoothing_length } => {
            Some(Box::new(Rsi::new(length, smoothing_length, stoch_length)))
        }
        IndicatorKind::StochRsi { length, k_smoothing, d_smoothing } => {
            Some(Box::new(StochRsi::new(length, k_smoothing, d_smoothing)))
        }
        IndicatorKind::Adx { periods, di_length } => {
            Some(Box::new(Adx::new(periods, di_length)))
        }
        IndicatorKind::Atr(period) => {
            Some(Box::new(Atr::new(period)))
        }
        IndicatorKind::Ema(period) => {
            Some(Box::new(Ema::new(period)))
        }
        IndicatorKind::EmaCross { short, long } => {
            Some(Box::new(EmaCross::new(short, long)))
        }
        IndicatorKind::Sma(period) => {
            Some(Box::new(Sma::new(period)))
        }
    }
}




pub struct Tracker{
    pub price_data: VecDeque<Price>,
    pub indicators: HashMap<IndicatorKind, Handler>,
    tf: TimeFrame,
    next_close: u64,
    init: bool,
}



impl Tracker{
    pub fn new(tf: TimeFrame) -> Self{
        Tracker{
            price_data: Vec::with_capacity(MAX_HISTORY),
            indicators: HashMap::new(),
            tf,
            next_close: 0_u64,
            init: false,
        }
    }


    pub fn digest(&mut self, data: Price){
        let time = get_time_now(); 
        if !self.init{
            self.update_indicators(price, false);
            self.init = true;
            return;
        };

        if time >= self.next_close{
            self.update_indicators(price, true);
            self.calc_next_close();
        }else{
            self.update_indicators(price, false);
        }
        
    }

    fn update_indicators(&mut self,price: Price, after_close: bool){

        for kind, handler in &mut self.indicators{
            handler.update(price, after_close);
        }
    }
    
    fn calc_next_close(&mut self) {
        let now = get_time_now();

        let tf_ms = self.tf.to_millis();
        self.next_close = ((now / tf_ms) + 1) * tf_ms;
    }
    
    
    pub fn load(&mut self, price_data: &Vec<Price>){
        
        for _kind, hanlder in &mut self.indicators{
            handler.load(price_data);
        }

        self.price_data
    }


    pub fn add_indicator(&mut self, kind: IndicatorKind){
        let mut handler = Handler::new(kind);
        handler.load(&self.price_data);
        self.indicators.insert(kind, handler);
    }

    pub fn toggle_indicator(&mut self, kind: IndicatorKind){
        if let Some(handler) = self.indicators.get_mut(kind){
            let _ = handler.toggle();
        }
    }
    
    pub fn reset(&mut self){
        self.price_data.clear();
        for _kind, handler in &mut self.indicators{
            handler.reset();
        }
        self.init = false;
    }

}














