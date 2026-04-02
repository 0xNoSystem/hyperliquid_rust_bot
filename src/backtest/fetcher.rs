use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::warn;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep};

use super::candle_store::{CandleKey, CandleStore};
use crate::{Error, Price, TimeFrame};

const MAX_HTTP_RETRIES: usize = 5;
const RETRY_BASE_DELAY_MS: u64 = 500;
const RETRY_MAX_DELAY_MS: u64 = 20_000;
const RETRY_JITTER_MS: u64 = 250;

#[derive(Clone)]
pub(crate) struct RequestLimiter {
    interval: Duration,
    next_allowed: Arc<Mutex<Instant>>,
}

impl RequestLimiter {
    pub(crate) fn from_requests_per_second(rps: u32) -> Option<Self> {
        if rps == 0 {
            return None;
        }
        Some(Self {
            interval: Duration::from_secs_f64(1.0 / rps as f64),
            next_allowed: Arc::new(Mutex::new(Instant::now())),
        })
    }

    pub(crate) async fn acquire(&self) {
        let wait_for = {
            let mut next = self.next_allowed.lock().await;
            let now = Instant::now();
            if now >= *next {
                *next = now + self.interval;
                None
            } else {
                let wait = *next - now;
                *next += self.interval;
                Some(wait)
            }
        };

        if let Some(delay) = wait_for {
            sleep(delay).await;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarketType {
    Spot,
    Futures,
}

impl MarketType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MarketType::Spot => "spot",
            MarketType::Futures => "futures",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Exchange {
    Binance,
    Bybit,
    Htx,
}

impl Exchange {
    pub fn name(&self) -> &'static str {
        match self {
            Exchange::Binance => "Binance",
            Exchange::Bybit => "Bybit",
            Exchange::Htx => "HTX",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSource {
    pub exchange: Exchange,
    pub market: MarketType,
    pub quote_asset: String,
}

impl Default for DataSource {
    fn default() -> Self {
        DataSource {
            exchange: Exchange::Binance,
            market: MarketType::Futures,
            quote_asset: "USDT".to_string(),
        }
    }
}

impl DataSource {
    pub const DEFAULT_QUOTE: &'static str = "USDT";

    pub fn new(exchange: Exchange, market: MarketType) -> Self {
        Self {
            exchange,
            market,
            quote_asset: Self::DEFAULT_QUOTE.to_string(),
        }
    }

    pub fn with_quote(
        exchange: Exchange,
        market: MarketType,
        quote_asset: impl Into<String>,
    ) -> Self {
        let quote_asset = Self::normalize_quote(quote_asset.into());
        Self {
            exchange,
            market,
            quote_asset,
        }
    }

    pub fn set_quote_asset(&mut self, quote_asset: impl Into<String>) {
        self.quote_asset = Self::normalize_quote(quote_asset.into());
    }

    pub fn candle_key(&self, asset: &str, tf: TimeFrame) -> CandleKey {
        CandleKey {
            exchange: self.exchange.name().to_uppercase(),
            market: self.market.as_str().to_uppercase(),
            asset_quote: format!(
                "{}_{}",
                asset.trim().to_uppercase(),
                self.quote_asset.to_uppercase()
            ),
            tf: tf.to_string().to_uppercase(),
        }
    }

    fn normalize_quote(quote_asset: String) -> String {
        let trimmed = quote_asset.trim();
        if trimmed.is_empty() {
            Self::DEFAULT_QUOTE.to_string()
        } else {
            trimmed.to_uppercase()
        }
    }

    fn interval_plan(&self, tf: TimeFrame) -> Result<IntervalPlan, Error> {
        let map = self.interval_map()?;

        if let Some((_, interval)) = map.iter().find(|(candidate, _)| *candidate == tf) {
            return Ok(IntervalPlan {
                interval,
                base_tf: tf,
                group_size: 1,
            });
        }

        let target_secs = tf.to_secs();
        let mut best: Option<(TimeFrame, &'static str)> = None;
        for (candidate, interval) in map.iter().copied() {
            let base_secs = candidate.to_secs();
            if base_secs > target_secs || !target_secs.is_multiple_of(base_secs) {
                continue;
            }
            let replace = match best {
                None => true,
                Some((best_tf, _)) => base_secs > best_tf.to_secs(),
            };
            if replace {
                best = Some((candidate, interval));
            }
        }

        let (base_tf, interval) = best.ok_or_else(|| {
            Error::Custom(format!(
                "Timeframe {tf} not supported for {} {:?}",
                self.exchange.name(),
                self.market
            ))
        })?;

        Ok(IntervalPlan {
            interval,
            base_tf,
            group_size: target_secs / base_tf.to_secs(),
        })
    }

    fn interval_map(&self) -> Result<&'static [(TimeFrame, &'static str)], Error> {
        const BINANCE: &[(TimeFrame, &str)] = &[
            (TimeFrame::Min1, "1m"),
            (TimeFrame::Min3, "3m"),
            (TimeFrame::Min5, "5m"),
            (TimeFrame::Min15, "15m"),
            (TimeFrame::Min30, "30m"),
            (TimeFrame::Hour1, "1h"),
            (TimeFrame::Hour2, "2h"),
            (TimeFrame::Hour4, "4h"),
            (TimeFrame::Hour12, "12h"),
            (TimeFrame::Day1, "1d"),
            (TimeFrame::Day3, "3d"),
            (TimeFrame::Week, "1w"),
            (TimeFrame::Month, "1M"),
        ];
        const BYBIT: &[(TimeFrame, &str)] = &[
            (TimeFrame::Min1, "1"),
            (TimeFrame::Min3, "3"),
            (TimeFrame::Min5, "5"),
            (TimeFrame::Min15, "15"),
            (TimeFrame::Min30, "30"),
            (TimeFrame::Hour1, "60"),
            (TimeFrame::Hour2, "120"),
            (TimeFrame::Hour4, "240"),
            (TimeFrame::Hour12, "720"),
            (TimeFrame::Day1, "D"),
            (TimeFrame::Week, "W"),
            (TimeFrame::Month, "M"),
        ];
        const HTX: &[(TimeFrame, &str)] = &[
            (TimeFrame::Min1, "1min"),
            (TimeFrame::Min5, "5min"),
            (TimeFrame::Min15, "15min"),
            (TimeFrame::Min30, "30min"),
            (TimeFrame::Hour1, "60min"),
            (TimeFrame::Hour4, "4hour"),
            (TimeFrame::Day1, "1day"),
            (TimeFrame::Week, "1week"),
            (TimeFrame::Month, "1mon"),
        ];
        let map = match self.exchange {
            Exchange::Binance => BINANCE,
            Exchange::Bybit => BYBIT,
            Exchange::Htx => HTX,
        };

        Ok(map)
    }

    fn request_limit(&self) -> Option<usize> {
        match (self.exchange, self.market) {
            (Exchange::Binance, MarketType::Spot) => Some(1000),
            (Exchange::Binance, MarketType::Futures) => Some(1500),
            (Exchange::Bybit, _) => Some(1000),
            _ => None,
        }
    }

    fn build_url(
        &self,
        asset: &str,
        base_tf: TimeFrame,
        interval: &'static str,
        start: u64,
        end: u64,
    ) -> Result<String, Error> {
        let symbol = self.format_asset(asset)?;
        let (start, end) = self.format_start_end(start, end);

        let url = match self.exchange {
            Exchange::Binance => match self.market {
                MarketType::Spot => format!(
                    "https://api.binance.com/api/v3/klines?symbol={symbol}&interval={interval}&startTime={start}&endTime={end}&limit=1000"
                ),
                MarketType::Futures => format!(
                    "https://fapi.binance.com/fapi/v1/klines?symbol={symbol}&interval={interval}&startTime={start}&endTime={end}&limit=1500"
                ),
            },
            Exchange::Bybit => {
                let category = match self.market {
                    MarketType::Spot => "spot",
                    MarketType::Futures => "linear",
                };
                format!(
                    "https://api.bybit.com/v5/market/kline?category={category}&symbol={symbol}&interval={interval}&start={start}&end={end}&limit=1000"
                )
            }
            Exchange::Htx => {
                let base_ms = base_tf.to_millis();
                let size = Self::calc_htx_size(start, end, base_ms);
                let base_url = match self.market {
                    MarketType::Spot => "https://api.huobi.pro/market/history/kline",
                    MarketType::Futures => {
                        "https://api.hbdm.com/linear-swap-ex/market/history/kline"
                    }
                };
                let symbol_key = match self.market {
                    MarketType::Spot => "symbol",
                    MarketType::Futures => "contract_code",
                };
                format!("{base_url}?{symbol_key}={symbol}&period={interval}&size={size}")
            }
        };

        Ok(url)
    }

    fn format_asset(&self, asset: &str) -> Result<String, Error> {
        let (separator, lowercase) = match (self.exchange, self.market) {
            (Exchange::Binance, _) | (Exchange::Bybit, _) => ("", false),
            (Exchange::Htx, MarketType::Spot) => ("", true),
            (Exchange::Htx, MarketType::Futures) => ("-", false),
        };

        let base = asset.trim().to_uppercase();
        let quote = self.quote_asset.as_str();
        let has_separator = base.contains('-') || base.contains('_');
        let has_quote = base.contains(quote);
        let symbol = if has_separator || has_quote {
            base
        } else {
            format!("{}{}{}", base, separator, quote)
        };

        if lowercase {
            Ok(symbol.to_lowercase())
        } else {
            Ok(symbol)
        }
    }

    fn format_start_end(&self, start: u64, end: u64) -> (u64, u64) {
        (start, end)
    }

    fn parse_candles(&self, body: &str, base_tf: TimeFrame) -> Result<Vec<Price>, Error> {
        let interval_ms = base_tf.to_millis();
        let json: Value =
            serde_json::from_str(body).map_err(|e| Error::Custom(format!("Invalid JSON: {e}")))?;

        match self.exchange {
            Exchange::Binance => parse_binance_like(&json, interval_ms),
            Exchange::Bybit => parse_bybit(&json, interval_ms),
            Exchange::Htx => parse_htx(&json, interval_ms),
        }
    }

    fn calc_htx_size(start: u64, end: u64, base_interval_ms: u64) -> u64 {
        let span = end.saturating_sub(start);
        let mut size = span
            .checked_add(base_interval_ms - 1)
            .map(|v| v / base_interval_ms)
            .unwrap_or(1);
        size = size.saturating_add(10);
        size.clamp(1, 2000)
    }
}

#[derive(Debug, Clone, Copy)]
struct IntervalPlan {
    interval: &'static str,
    base_tf: TimeFrame,
    group_size: u64,
}

pub struct Fetcher {
    client: Client,
    pub current_source: DataSource,
    store: Arc<CandleStore>,
    request_limiter: Option<RequestLimiter>,
}

impl Fetcher {
    pub fn new(current_source: DataSource, store: Arc<CandleStore>) -> Self {
        Self {
            client: Client::new(),
            current_source,
            store,
            request_limiter: None,
        }
    }

    pub fn set_source(&mut self, source: DataSource) {
        self.current_source = source;
    }

    pub(crate) fn set_request_limiter(&mut self, limiter: Option<RequestLimiter>) {
        self.request_limiter = limiter;
    }

    pub async fn fetch(
        &mut self,
        asset: &str,
        tf: TimeFrame,
        start: u64,
        end: u64,
    ) -> Result<Vec<Price>, Error> {
        self.fetch_with_progress(asset, tf, start, end, |_, _| {})
            .await
    }

    pub async fn fetch_with_progress<F>(
        &mut self,
        asset: &str,
        tf: TimeFrame,
        start: u64,
        end: u64,
        mut on_progress: F,
    ) -> Result<Vec<Price>, Error>
    where
        F: FnMut(u64, u64),
    {
        let asset = asset.trim().to_uppercase();
        if asset.is_empty() {
            return Ok(Vec::new());
        }
        if end <= start {
            return Err(Error::Custom(
                "Invalid time range: end must be greater than start".to_string(),
            ));
        }

        let candle_interval_ms = tf.to_millis();
        let range_start = start;
        let mut range_end = end;
        let now = now_ms();
        if range_end > now {
            range_end = now;
        }
        if range_end <= range_start {
            return Err(Error::Custom(
                "Requested range is outside available historical data".to_string(),
            ));
        }

        let (normalized_start, normalized_end) =
            normalize_range(range_start, range_end, candle_interval_ms);
        let estimated_total =
            estimate_points_in_range(normalized_start, normalized_end, candle_interval_ms).max(1);

        let candle_key = self.current_source.candle_key(&asset, tf);

        // Acquire per-key lock. If another task is already fetching this key,
        // we subscribe to its progress and wait — avoiding duplicate HTTP calls.
        let guard = self
            .store
            .acquire_key(&candle_key, |loaded, total| {
                on_progress(loaded, total);
            })
            .await;

        // If we waited on another fetcher, data should now be cached.
        // Do a fresh lookup either way.
        let lookup = self.store.lookup_range(
            &candle_key,
            normalized_start,
            normalized_end,
            candle_interval_ms,
        );
        let missing = lookup.missing;
        let cached = lookup.cached;
        let cached_in_range = lookup.cached_in_range;
        on_progress(cached_in_range, estimated_total);

        if let Some(values) = cached {
            on_progress(
                values.len() as u64,
                estimated_total.max(values.len() as u64),
            );
            return Ok(values);
        }

        if !guard.is_first() {
            // We waited but data is still incomplete — partial overlap.
            // Fall through to fetch remaining segments while holding no lock.
        }

        let mut loaded = cached_in_range.min(estimated_total);
        for segment in missing {
            let segment_total =
                estimate_points_in_range(segment.start, segment.end, candle_interval_ms);
            let base_loaded = loaded;
            let data = self
                .fetch_segment(&asset, tf, segment.start, segment.end, |segment_loaded| {
                    let progress = base_loaded
                        .saturating_add(segment_loaded.min(segment_total))
                        .min(estimated_total);
                    on_progress(progress, estimated_total);
                    guard.send_progress(progress, estimated_total);
                })
                .await?;
            self.store.insert_many(&candle_key, &data);
            loaded = self
                .store
                .count_range(&candle_key, normalized_start, normalized_end);
            on_progress(loaded.min(estimated_total), estimated_total);
            guard.send_progress(loaded.min(estimated_total), estimated_total);
        }

        let out = self
            .store
            .range_to_vec(&candle_key, normalized_start, normalized_end);
        on_progress(out.len() as u64, estimated_total.max(out.len() as u64));
        Ok(out)
    }

    async fn fetch_segment<F>(
        &self,
        asset: &str,
        tf: TimeFrame,
        start: u64,
        end: u64,
        mut on_segment_progress: F,
    ) -> Result<Vec<Price>, Error>
    where
        F: FnMut(u64),
    {
        let plan = self.current_source.interval_plan(tf)?;
        let base_interval_ms = plan.base_tf.to_millis();

        let mut collected: Vec<Price> = match self.current_source.exchange {
            Exchange::Binance => {
                let limit = self.current_source.request_limit().unwrap_or(1000);
                let mut cursor = start;
                let mut out = Vec::new();
                let mut loaded = 0_u64;
                while cursor < end {
                    let data = self
                        .fetch_once(asset, plan.base_tf, plan.interval, cursor, end)
                        .await?;
                    if data.is_empty() {
                        break;
                    }
                    let last_start = data.iter().map(|p| p.open_time).max().unwrap_or(cursor);
                    let count = data.len();
                    out.extend(data);
                    loaded = loaded.saturating_add(count as u64);
                    on_segment_progress(loaded);
                    if last_start <= cursor {
                        break;
                    }
                    cursor = last_start + 1;
                    if count < limit {
                        break;
                    }
                }
                out
            }
            Exchange::Bybit => {
                let limit = self.current_source.request_limit().unwrap_or(1000);
                let mut cursor = start;
                let mut out = Vec::new();
                let mut loaded = 0_u64;
                while cursor < end {
                    let data = self
                        .fetch_once(asset, plan.base_tf, plan.interval, cursor, end)
                        .await?;
                    if data.is_empty() {
                        break;
                    }
                    let max_start = data.iter().map(|p| p.open_time).max().unwrap_or(cursor);
                    let count = data.len();
                    out.extend(data);
                    loaded = loaded.saturating_add(count as u64);
                    on_segment_progress(loaded);
                    let next = max_start.saturating_add(base_interval_ms);
                    if next <= cursor {
                        break;
                    }
                    cursor = next;
                    if count < limit {
                        break;
                    }
                }
                out
            }
            Exchange::Htx => {
                let out = self
                    .fetch_once(asset, plan.base_tf, plan.interval, start, end)
                    .await?;
                on_segment_progress(out.len() as u64);
                out
            }
        };

        collected.retain(|p| p.close_time > start && p.open_time < end);

        let mut map: BTreeMap<u64, Price> = BTreeMap::new();
        for price in collected.drain(..) {
            map.insert(price.open_time, price);
        }
        let mut ordered: Vec<Price> = map.into_values().collect();

        if plan.group_size > 1 {
            ordered = aggregate_prices(&ordered, tf.to_millis());
        }

        Ok(ordered)
    }

    async fn fetch_once(
        &self,
        asset: &str,
        base_tf: TimeFrame,
        interval: &'static str,
        start: u64,
        end: u64,
    ) -> Result<Vec<Price>, Error> {
        let url = self
            .current_source
            .build_url(asset, base_tf, interval, start, end)?;

        let body = self.request_body(&url).await?;
        self.current_source.parse_candles(&body, base_tf)
    }

    async fn request_body(&self, url: &str) -> Result<String, Error> {
        for attempt in 0..=MAX_HTTP_RETRIES {
            if let Some(limiter) = &self.request_limiter {
                limiter.acquire().await;
            }

            let response = match self.client.get(url).send().await {
                Ok(response) => response,
                Err(e) => {
                    if attempt < MAX_HTTP_RETRIES {
                        let delay = retry_delay_for_attempt(attempt, None);
                        warn!(
                            "HTTP transport error for {} (attempt {}/{}): {}. Retrying in {}ms",
                            url,
                            attempt + 1,
                            MAX_HTTP_RETRIES + 1,
                            e,
                            delay.as_millis()
                        );
                        sleep(delay).await;
                        continue;
                    }
                    warn!(
                        "HTTP transport error for {} after {} attempts: {}",
                        url,
                        MAX_HTTP_RETRIES + 1,
                        e
                    );
                    return Err(Error::Custom(format!("Request failed: {e}")));
                }
            };

            let status = response.status();
            if status.is_success() {
                return response.text().await.map_err(|e| {
                    warn!("Failed to read HTTP response body for {url}: {e}");
                    Error::Custom(format!("Failed to read response: {e}"))
                });
            }

            let retry_after = parse_retry_after_header(response.headers());
            let should_retry =
                status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
            let body = response.text().await.unwrap_or_default();
            let body_preview = truncate_for_log(&body, 240);

            if should_retry && attempt < MAX_HTTP_RETRIES {
                let delay = retry_delay_for_attempt(attempt, retry_after);
                warn!(
                    "HTTP {} for {} (attempt {}/{}). Retrying in {}ms{}{}",
                    status,
                    url,
                    attempt + 1,
                    MAX_HTTP_RETRIES + 1,
                    delay.as_millis(),
                    retry_after
                        .map(|d| format!(" (Retry-After={}s)", d.as_secs()))
                        .unwrap_or_default(),
                    if body_preview.is_empty() {
                        String::new()
                    } else {
                        format!(" body={}", body_preview)
                    }
                );
                sleep(delay).await;
                continue;
            }

            warn!(
                "HTTP request returned non-success status {} for {}{}",
                status,
                url,
                if body_preview.is_empty() {
                    String::new()
                } else {
                    format!(" body={}", body_preview)
                }
            );
            return Err(Error::Custom(format!(
                "Request failed with status {}",
                status
            )));
        }

        Err(Error::Custom(
            "Request failed after retries with unknown error".to_string(),
        ))
    }
}

fn normalize_range(start_ms: u64, end_ms: u64, candle_interval_ms: u64) -> (u64, u64) {
    let normalized_start = start_ms.saturating_sub(start_ms % candle_interval_ms);
    let normalized_end = std::cmp::max(
        normalized_start.saturating_add(candle_interval_ms),
        div_ceil(end_ms, candle_interval_ms).saturating_mul(candle_interval_ms),
    );

    (normalized_start, normalized_end)
}

fn estimate_points_in_range(start: u64, end: u64, step: u64) -> u64 {
    if end <= start {
        return 0;
    }
    std::cmp::max(1, div_ceil(end - start, step))
}

fn aggregate_prices(prices: &[Price], target_ms: u64) -> Vec<Price> {
    if prices.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<Price> = Vec::new();
    let mut current_start: Option<u64> = None;
    let mut bucket: Option<Price> = None;

    for price in prices {
        let start = (price.open_time / target_ms) * target_ms;
        if current_start != Some(start) {
            if let Some(existing) = bucket.take() {
                out.push(existing);
            }
            current_start = Some(start);
            bucket = Some(Price {
                open: price.open,
                high: price.high,
                low: price.low,
                close: price.close,
                open_time: start,
                close_time: start + target_ms,
                vlm: price.vlm,
            });
        } else if let Some(ref mut existing) = bucket {
            if price.high > existing.high {
                existing.high = price.high;
            }
            if price.low < existing.low {
                existing.low = price.low;
            }
            existing.close = price.close;
            existing.vlm += price.vlm;
        }
    }

    if let Some(existing) = bucket {
        out.push(existing);
    }

    out
}

fn parse_retry_after_header(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?;
    let raw = value.to_str().ok()?.trim();
    let secs = raw.parse::<u64>().ok()?;
    Some(Duration::from_secs(secs.clamp(1, 120)))
}

fn retry_delay_for_attempt(attempt: usize, retry_after: Option<Duration>) -> Duration {
    if let Some(delay) = retry_after {
        return delay;
    }

    let factor = 1_u64 << attempt.min(8);
    let exp = RETRY_BASE_DELAY_MS.saturating_mul(factor);
    let capped = exp.min(RETRY_MAX_DELAY_MS);
    let jitter = jitter_ms(RETRY_JITTER_MS);
    Duration::from_millis(capped.saturating_add(jitter))
}

fn jitter_ms(max_ms: u64) -> u64 {
    if max_ms == 0 {
        return 0;
    }
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => (d.subsec_nanos() as u64) % max_ms,
        Err(_) => 0,
    }
}

fn truncate_for_log(input: &str, max_chars: usize) -> String {
    if max_chars == 0 || input.is_empty() {
        return String::new();
    }
    let normalized = input.replace(['\n', '\r'], " ");
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        let mut out: String = normalized.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}

fn parse_binance_like(json: &Value, interval_ms: u64) -> Result<Vec<Price>, Error> {
    let list = json
        .as_array()
        .ok_or_else(|| Error::Custom("Expected array response".to_string()))?;

    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let arr = item
            .as_array()
            .ok_or_else(|| Error::Custom("Expected array candle".to_string()))?;
        if arr.len() < 6 {
            return Err(Error::Custom("Invalid candle format".to_string()));
        }
        let start = parse_u64(&arr[0])?;
        let open = parse_f64(&arr[1])?;
        let high = parse_f64(&arr[2])?;
        let low = parse_f64(&arr[3])?;
        let close = parse_f64(&arr[4])?;
        let volume = parse_f64(&arr[5])?;
        let close_time = arr.get(6).and_then(|v| parse_u64(v).ok());
        out.push(build_price(
            start,
            open,
            high,
            low,
            close,
            volume,
            interval_ms,
            close_time,
        ));
    }

    Ok(out)
}

fn parse_bybit(json: &Value, interval_ms: u64) -> Result<Vec<Price>, Error> {
    let list = json
        .get("result")
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Custom("Missing result.list".to_string()))?;

    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let arr = item
            .as_array()
            .ok_or_else(|| Error::Custom("Expected array candle".to_string()))?;
        if arr.len() < 6 {
            return Err(Error::Custom("Invalid candle format".to_string()));
        }
        let start = parse_u64(&arr[0])?;
        let open = parse_f64(&arr[1])?;
        let high = parse_f64(&arr[2])?;
        let low = parse_f64(&arr[3])?;
        let close = parse_f64(&arr[4])?;
        let volume = parse_f64(&arr[5])?;
        out.push(build_price(
            start,
            open,
            high,
            low,
            close,
            volume,
            interval_ms,
            None,
        ));
    }

    Ok(out)
}

fn parse_htx(json: &Value, interval_ms: u64) -> Result<Vec<Price>, Error> {
    let list = json
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Custom("Missing data".to_string()))?;

    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let obj = item
            .as_object()
            .ok_or_else(|| Error::Custom("Expected object candle".to_string()))?;
        let id = obj
            .get("id")
            .ok_or_else(|| Error::Custom("Missing id".to_string()))?;
        let start = parse_u64(id)? * 1000;
        let open = parse_f64(
            obj.get("open")
                .ok_or_else(|| Error::Custom("Missing open".to_string()))?,
        )?;
        let high = parse_f64(
            obj.get("high")
                .ok_or_else(|| Error::Custom("Missing high".to_string()))?,
        )?;
        let low = parse_f64(
            obj.get("low")
                .ok_or_else(|| Error::Custom("Missing low".to_string()))?,
        )?;
        let close = parse_f64(
            obj.get("close")
                .ok_or_else(|| Error::Custom("Missing close".to_string()))?,
        )?;
        let volume = obj
            .get("vol")
            .or_else(|| obj.get("amount"))
            .map(parse_f64)
            .transpose()?
            .unwrap_or(0.0);
        out.push(build_price(
            start,
            open,
            high,
            low,
            close,
            volume,
            interval_ms,
            None,
        ));
    }

    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn build_price(
    start: u64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    interval_ms: u64,
    close_override: Option<u64>,
) -> Price {
    Price {
        open,
        high,
        low,
        close,
        open_time: start,
        close_time: close_override.unwrap_or(start + interval_ms),
        vlm: volume,
    }
}

fn parse_u64(value: &Value) -> Result<u64, Error> {
    if let Some(n) = value.as_u64() {
        return Ok(n);
    }
    if let Some(s) = value.as_str() {
        return s
            .parse::<u64>()
            .map_err(|_| Error::Custom("Invalid integer".to_string()));
    }
    if let Some(f) = value.as_f64()
        && f.is_finite()
        && f >= 0.0
    {
        return Ok(f as u64);
    }
    Err(Error::Custom("Invalid integer".to_string()))
}

fn parse_f64(value: &Value) -> Result<f64, Error> {
    if let Some(n) = value.as_f64() {
        return Ok(n);
    }
    if let Some(s) = value.as_str() {
        return s
            .parse::<f64>()
            .map_err(|_| Error::Custom("Invalid float".to_string()));
    }
    Err(Error::Custom("Invalid float".to_string()))
}

fn div_ceil(value: u64, divisor: u64) -> u64 {
    if divisor == 0 {
        return 0;
    }
    value / divisor + u64::from(!value.is_multiple_of(divisor))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_candle_key_includes_quote() {
        let source = DataSource::with_quote(Exchange::Binance, MarketType::Spot, "usdc");
        let key = source.candle_key("btc", TimeFrame::Hour1);
        assert_eq!(key.exchange, "BINANCE");
        assert_eq!(key.market, "SPOT");
        assert_eq!(key.asset_quote, "BTC_USDC");
        assert_eq!(key.tf, "1H");
    }

    #[test]
    fn test_format_asset_quotes() {
        let binance = DataSource::with_quote(Exchange::Binance, MarketType::Spot, "usdc");
        assert_eq!(binance.format_asset("btc").unwrap(), "BTCUSDC");

        let htx = DataSource::with_quote(Exchange::Htx, MarketType::Spot, "usdt");
        assert_eq!(htx.format_asset("btc").unwrap(), "btcusdt");
    }

    #[test]
    fn test_parse_binance_like() {
        let json = json!([[0, "1", "2", "0.5", "1.5", "10", 500]]);
        let out = parse_binance_like(&json, 60_000).unwrap();
        assert_eq!(out.len(), 1);
        let price = out[0];
        assert_eq!(price.open_time, 0);
        assert_eq!(price.close_time, 500);
        assert_eq!(price.open, 1.0);
        assert_eq!(price.high, 2.0);
        assert_eq!(price.low, 0.5);
        assert_eq!(price.close, 1.5);
        assert_eq!(price.vlm, 10.0);
    }

    #[test]
    fn test_parse_bybit() {
        let json = json!({"result": {"list": [["0", "1", "2", "0.5", "1.5", "10"]]}});
        let out = parse_bybit(&json, 60_000).unwrap();
        assert_eq!(out.len(), 1);
        let price = out[0];
        assert_eq!(price.open_time, 0);
        assert_eq!(price.close_time, 60_000);
        assert_eq!(price.open, 1.0);
        assert_eq!(price.high, 2.0);
        assert_eq!(price.low, 0.5);
        assert_eq!(price.close, 1.5);
        assert_eq!(price.vlm, 10.0);
    }

    #[test]
    fn test_parse_htx() {
        let json = json!({
            "data": [
                {"id": 1, "open": 1, "high": 2, "low": 0.5, "close": 1.5, "vol": 10}
            ]
        });
        let out = parse_htx(&json, 60_000).unwrap();
        assert_eq!(out.len(), 1);
        let price = out[0];
        assert_eq!(price.open_time, 1000);
        assert_eq!(price.close_time, 61_000);
        assert_eq!(price.high, 2.0);
        assert_eq!(price.vlm, 10.0);
    }
}
