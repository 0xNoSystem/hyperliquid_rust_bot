

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
    tf: TimeFrame,
    is_active: bool,
}

impl Handler{

    pub fn new(indicator: IndicatorKind,tf: TimeFrame) -> Handler{
        Handler{
            indicator: match_kind(indicator),
            tf,
            is_active: true,
        }
    }

    pub fn from_index_id((kind, tf): IndexId) -> Self{
        Self{
            indicator: match_kind(kind),
            tf,
            is_active: true,
        }
    }
    pub fn update(&mut self, after_close: bool){
        if after_close{
            self.indicator.update_after_close();
        }else{
            self.indicator.update_before_close();
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

    



type IndexId = (IndicatorKind, TimeFrame);

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









