use crate::config::Config;
use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const STATS_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageRange {
    Live,
    Days7,
    Days30,
    All,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub provider: String,
    pub requested_model: String,
    pub final_model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usage_available: bool,
    pub retry_count: u32,
    pub failed: bool,
    pub at: DateTime<Local>,
}

impl UsageRecord {
    pub fn success(
        provider: impl Into<String>,
        model: impl Into<String>,
        input_tokens: u64,
        output_tokens: u64,
        retry_count: u32,
        usage_available: bool,
    ) -> Self {
        let model = model.into();
        Self {
            provider: provider.into(),
            requested_model: model.clone(),
            final_model: model,
            input_tokens,
            output_tokens,
            usage_available,
            retry_count,
            failed: false,
            at: Local::now(),
        }
    }

    pub fn failure(
        provider: impl Into<String>,
        model: impl Into<String>,
        retry_count: u32,
        usage_available: bool,
    ) -> Self {
        let model = model.into();
        Self {
            provider: provider.into(),
            requested_model: model.clone(),
            final_model: model,
            input_tokens: 0,
            output_tokens: 0,
            usage_available,
            retry_count,
            failed: true,
            at: Local::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageStatsSnapshot {
    pub range: UsageRange,
    pub generated_at: String,
    pub totals: Aggregate,
    pub trend: Vec<TrendPoint>,
    pub models: Vec<ModelAggregate>,
    pub providers: Vec<ProviderAggregate>,
    pub history_recovered: bool,
    pub history_writable: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Aggregate {
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub failed_requests: u64,
    pub retry_count: u64,
    pub success_rate: f64,
    pub usage_missing_requests: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrendPoint {
    pub label: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub failed_requests: u64,
    pub retry_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelAggregate {
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub failed_requests: u64,
    pub retry_count: u64,
    pub usage_missing_requests: u64,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAggregate {
    pub provider: String,
    pub requests: u64,
    pub total_tokens: u64,
    pub failed_requests: u64,
    pub retry_count: u64,
}

#[allow(dead_code)]
pub struct UsageStatsState {
    store: Arc<UsageStatsStore>,
}

#[allow(dead_code)]
impl UsageStatsState {
    pub fn new() -> Self {
        let path = Config::config_dir().join("usage-stats.json");
        let store = UsageStatsStore::from_path(path.clone())
            .unwrap_or_else(|_| UsageStatsStore::unwritable(path));
        Self {
            store: Arc::new(store),
        }
    }

    pub fn store(&self) -> Arc<UsageStatsStore> {
        Arc::clone(&self.store)
    }
}

#[derive(Debug, Clone)]
pub struct StatsWriteError {
    path: PathBuf,
    message: String,
}

impl StatsWriteError {
    fn new(path: PathBuf, message: impl Into<String>) -> Self {
        Self {
            path,
            message: message.into(),
        }
    }
}

impl fmt::Display for StatsWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to write usage stats at {}: {}",
            self.path.display(),
            self.message
        )
    }
}

impl std::error::Error for StatsWriteError {}

pub struct UsageStatsStore {
    path: Option<PathBuf>,
    inner: Mutex<StoreInner>,
}

impl UsageStatsStore {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            inner: Mutex::new(StoreInner::default()),
        }
    }

    pub fn from_path(path: PathBuf) -> Result<Self, StatsWriteError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| StatsWriteError::new(path.clone(), error.to_string()))?;
        }

        let mut document = StatsDocument::default();
        let mut history_recovered = false;
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<StatsDocument>(&bytes) {
                Ok(loaded) => {
                    document = loaded;
                }
                Err(_) => {
                    history_recovered = true;
                    rename_corrupt_file(&path);
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(StatsWriteError::new(path.clone(), error.to_string()));
            }
        }

        document.version = STATS_DOCUMENT_VERSION;

        Ok(Self {
            path: Some(path),
            inner: Mutex::new(StoreInner {
                document,
                history_recovered,
                history_writable: true,
                ..StoreInner::default()
            }),
        })
    }

    pub fn record(&self, record: UsageRecord) -> Result<(), StatsWriteError> {
        self.record_inner(record)
    }

    pub fn record_at(
        &self,
        mut record: UsageRecord,
        at: DateTime<Local>,
    ) -> Result<(), StatsWriteError> {
        record.at = at;
        self.record_inner(record)
    }

    pub fn snapshot(&self, range: UsageRange) -> UsageStatsSnapshot {
        let snapshot_state = {
            let inner = self.inner.lock().unwrap();
            SnapshotState {
                document: inner.document.clone(),
                live_totals: inner.live_totals.clone(),
                live_models: inner.live_models.clone(),
                live_providers: inner.live_providers.clone(),
                live_quarter_hours: inner.live_quarter_hours.clone(),
                live_days: inner.live_days.clone(),
                history_recovered: inner.history_recovered,
                history_writable: inner.history_writable,
            }
        };

        let today = Local::now().date_naive();
        let (totals, trend, models, providers) = match range {
            UsageRange::Live => build_live_snapshot_parts(
                &snapshot_state.live_totals,
                &snapshot_state.live_models,
                &snapshot_state.live_providers,
                &snapshot_state.live_quarter_hours,
            ),
            UsageRange::Days7 | UsageRange::Days30 | UsageRange::All => {
                build_historical_snapshot_parts(range, &snapshot_state, today)
            }
        };

        UsageStatsSnapshot {
            range,
            generated_at: Local::now().to_rfc3339(),
            totals,
            trend,
            models,
            providers,
            history_recovered: snapshot_state.history_recovered,
            history_writable: snapshot_state.history_writable,
        }
    }

    #[allow(dead_code)]
    fn unwritable(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            inner: Mutex::new(StoreInner {
                history_writable: false,
                ..StoreInner::default()
            }),
        }
    }

    fn record_inner(&self, record: UsageRecord) -> Result<(), StatsWriteError> {
        let path = self.path.clone();
        let document_to_write = {
            let mut inner = self.inner.lock().unwrap();
            inner.live_totals.apply_record(&record);
            apply_record_to_provider_map(&mut inner.live_providers, &record);
            apply_record_to_model_map(&mut inner.live_models, &record);

            let quarter_label = quarter_hour_label(record.at);
            inner
                .live_quarter_hours
                .entry(quarter_label)
                .or_default()
                .apply_record(&record);

            let day_key = day_key(record.at);
            inner
                .live_days
                .entry(day_key.clone())
                .or_default()
                .apply_record(&record);
            inner
                .document
                .days
                .entry(day_key)
                .or_default()
                .apply_record(&record);

            inner.document.version = STATS_DOCUMENT_VERSION;
            if path.is_some() {
                Some(inner.document.clone())
            } else {
                inner.history_writable = true;
                None
            }
        };

        if let (Some(path), Some(document)) = (path, document_to_write) {
            if let Err(error) = write_stats_document(&path, &document) {
                self.inner.lock().unwrap().history_writable = false;
                return Err(error);
            }

            self.inner.lock().unwrap().history_writable = true;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct SnapshotState {
    document: StatsDocument,
    live_totals: Counter,
    live_models: BTreeMap<String, ModelCounter>,
    live_providers: BTreeMap<String, Counter>,
    live_quarter_hours: BTreeMap<String, Counter>,
    live_days: BTreeMap<String, DailyBucket>,
    history_recovered: bool,
    history_writable: bool,
}

#[derive(Debug, Clone, Default)]
struct StoreInner {
    document: StatsDocument,
    live_totals: Counter,
    live_models: BTreeMap<String, ModelCounter>,
    live_providers: BTreeMap<String, Counter>,
    live_quarter_hours: BTreeMap<String, Counter>,
    live_days: BTreeMap<String, DailyBucket>,
    history_recovered: bool,
    history_writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatsDocument {
    #[serde(default = "default_stats_document_version")]
    version: u32,
    #[serde(default)]
    days: BTreeMap<String, DailyBucket>,
}

impl Default for StatsDocument {
    fn default() -> Self {
        Self {
            version: STATS_DOCUMENT_VERSION,
            days: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DailyBucket {
    #[serde(default)]
    totals: Counter,
    #[serde(default)]
    trend: Counter,
    #[serde(default)]
    models: BTreeMap<String, ModelCounter>,
    #[serde(default)]
    providers: BTreeMap<String, Counter>,
}

impl DailyBucket {
    fn apply_record(&mut self, record: &UsageRecord) {
        self.totals.apply_record(record);
        self.trend.apply_record(record);
        apply_record_to_provider_map(&mut self.providers, record);
        apply_record_to_model_map(&mut self.models, record);
    }

    fn is_zero(&self) -> bool {
        self.totals.is_zero()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ModelCounter {
    provider: String,
    model: String,
    #[serde(flatten)]
    totals: Counter,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Counter {
    #[serde(default)]
    requests: u64,
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    failed_requests: u64,
    #[serde(default)]
    retry_count: u64,
    #[serde(default)]
    usage_missing_requests: u64,
}

impl Counter {
    fn apply_record(&mut self, record: &UsageRecord) {
        self.requests += 1;
        self.input_tokens += record.input_tokens;
        self.output_tokens += record.output_tokens;
        self.failed_requests += u64::from(record.failed);
        self.retry_count += u64::from(record.retry_count);
        self.usage_missing_requests += u64::from(!record.usage_available);
    }

    fn add_assign(&mut self, other: &Self) {
        self.requests += other.requests;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.failed_requests += other.failed_requests;
        self.retry_count += other.retry_count;
        self.usage_missing_requests += other.usage_missing_requests;
    }

    fn subtract_assign(&mut self, other: &Self) {
        self.requests = self.requests.saturating_sub(other.requests);
        self.input_tokens = self.input_tokens.saturating_sub(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_sub(other.output_tokens);
        self.failed_requests = self.failed_requests.saturating_sub(other.failed_requests);
        self.retry_count = self.retry_count.saturating_sub(other.retry_count);
        self.usage_missing_requests = self
            .usage_missing_requests
            .saturating_sub(other.usage_missing_requests);
    }

    fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    fn success_rate(&self) -> f64 {
        if self.requests == 0 {
            1.0
        } else {
            (self.requests - self.failed_requests) as f64 / self.requests as f64
        }
    }

    fn is_zero(&self) -> bool {
        self.requests == 0
            && self.input_tokens == 0
            && self.output_tokens == 0
            && self.failed_requests == 0
            && self.retry_count == 0
            && self.usage_missing_requests == 0
    }

    fn as_public_aggregate(&self) -> Aggregate {
        Aggregate {
            requests: self.requests,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            total_tokens: self.total_tokens(),
            failed_requests: self.failed_requests,
            retry_count: self.retry_count,
            success_rate: self.success_rate(),
            usage_missing_requests: self.usage_missing_requests,
        }
    }
}

#[derive(Default)]
struct SnapshotAccumulator {
    totals: Counter,
    models: BTreeMap<String, ModelCounter>,
    providers: BTreeMap<String, Counter>,
}

impl SnapshotAccumulator {
    fn add_bucket(&mut self, bucket: &DailyBucket) {
        self.totals.add_assign(&bucket.totals);
        for (provider, counter) in &bucket.providers {
            self.providers
                .entry(provider.clone())
                .or_default()
                .add_assign(counter);
        }
        for (key, model_counter) in &bucket.models {
            let entry = self.models.entry(key.clone()).or_insert_with(|| ModelCounter {
                provider: model_counter.provider.clone(),
                model: model_counter.model.clone(),
                totals: Counter::default(),
            });
            entry.totals.add_assign(&model_counter.totals);
        }
    }
}

fn default_stats_document_version() -> u32 {
    STATS_DOCUMENT_VERSION
}

fn build_live_snapshot_parts(
    live_totals: &Counter,
    live_models: &BTreeMap<String, ModelCounter>,
    live_providers: &BTreeMap<String, Counter>,
    live_quarter_hours: &BTreeMap<String, Counter>,
) -> (Aggregate, Vec<TrendPoint>, Vec<ModelAggregate>, Vec<ProviderAggregate>) {
    let totals = live_totals.as_public_aggregate();
    let mut trend = live_quarter_hours
        .iter()
        .map(|(label, counter)| counter_to_trend_point(label.clone(), counter))
        .collect::<Vec<_>>();
    trend.sort_by(|left, right| left.label.cmp(&right.label));

    let models = model_map_to_public(live_models);
    let providers = provider_map_to_public(live_providers);
    (totals, trend, models, providers)
}

fn build_historical_snapshot_parts(
    range: UsageRange,
    snapshot_state: &SnapshotState,
    today: NaiveDate,
) -> (Aggregate, Vec<TrendPoint>, Vec<ModelAggregate>, Vec<ProviderAggregate>) {
    let mut accumulator = SnapshotAccumulator::default();
    let mut trend = BTreeMap::<String, Counter>::new();

    for (day, bucket) in &snapshot_state.document.days {
        if !range_includes_day(range, today, day) {
            continue;
        }

        let effective = if let Some(live_day) = snapshot_state.live_days.get(day) {
            subtract_daily_bucket(bucket, live_day)
        } else {
            bucket.clone()
        };

        if effective.is_zero() {
            continue;
        }

        accumulator.add_bucket(&effective);
        trend.entry(day.clone()).or_default().add_assign(&effective.trend);
    }

    for (day, bucket) in &snapshot_state.live_days {
        if !range_includes_day(range, today, day) {
            continue;
        }

        accumulator.add_bucket(bucket);
        trend.entry(day.clone()).or_default().add_assign(&bucket.trend);
    }

    let mut trend_points = trend
        .into_iter()
        .filter(|(_, counter)| !counter.is_zero())
        .map(|(label, counter)| counter_to_trend_point(label, &counter))
        .collect::<Vec<_>>();
    trend_points.sort_by(|left, right| left.label.cmp(&right.label));

    let totals = accumulator.totals.as_public_aggregate();
    let models = model_map_to_public(&accumulator.models);
    let providers = provider_map_to_public(&accumulator.providers);
    (totals, trend_points, models, providers)
}

fn model_map_to_public(models: &BTreeMap<String, ModelCounter>) -> Vec<ModelAggregate> {
    let mut values = models
        .values()
        .map(|model_counter| ModelAggregate {
            provider: model_counter.provider.clone(),
            model: model_counter.model.clone(),
            requests: model_counter.totals.requests,
            input_tokens: model_counter.totals.input_tokens,
            output_tokens: model_counter.totals.output_tokens,
            total_tokens: model_counter.totals.total_tokens(),
            failed_requests: model_counter.totals.failed_requests,
            retry_count: model_counter.totals.retry_count,
            usage_missing_requests: model_counter.totals.usage_missing_requests,
            success_rate: model_counter.totals.success_rate(),
        })
        .collect::<Vec<_>>();

    values.sort_by(|left, right| {
        right
            .total_tokens
            .cmp(&left.total_tokens)
            .then_with(|| right.requests.cmp(&left.requests))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.model.cmp(&right.model))
    });
    values
}

fn provider_map_to_public(providers: &BTreeMap<String, Counter>) -> Vec<ProviderAggregate> {
    let mut values = providers
        .iter()
        .map(|(provider, counter)| ProviderAggregate {
            provider: provider.clone(),
            requests: counter.requests,
            total_tokens: counter.total_tokens(),
            failed_requests: counter.failed_requests,
            retry_count: counter.retry_count,
        })
        .collect::<Vec<_>>();

    values.sort_by(|left, right| {
        right
            .total_tokens
            .cmp(&left.total_tokens)
            .then_with(|| right.requests.cmp(&left.requests))
            .then_with(|| left.provider.cmp(&right.provider))
    });
    values
}

fn counter_to_trend_point(label: String, counter: &Counter) -> TrendPoint {
    TrendPoint {
        label,
        requests: counter.requests,
        input_tokens: counter.input_tokens,
        output_tokens: counter.output_tokens,
        total_tokens: counter.total_tokens(),
        failed_requests: counter.failed_requests,
        retry_count: counter.retry_count,
    }
}

fn subtract_daily_bucket(base: &DailyBucket, subtracted: &DailyBucket) -> DailyBucket {
    let mut result = base.clone();
    result.totals.subtract_assign(&subtracted.totals);
    result.trend.subtract_assign(&subtracted.trend);

    for (provider, counter) in &subtracted.providers {
        if let Some(existing) = result.providers.get_mut(provider) {
            existing.subtract_assign(counter);
            if existing.is_zero() {
                result.providers.remove(provider);
            }
        }
    }

    for (key, model_counter) in &subtracted.models {
        if let Some(existing) = result.models.get_mut(key) {
            existing.totals.subtract_assign(&model_counter.totals);
            if existing.totals.is_zero() {
                result.models.remove(key);
            }
        }
    }

    result
}

fn apply_record_to_provider_map(
    providers: &mut BTreeMap<String, Counter>,
    record: &UsageRecord,
) {
    providers
        .entry(record.provider.clone())
        .or_default()
        .apply_record(record);
}

fn apply_record_to_model_map(
    models: &mut BTreeMap<String, ModelCounter>,
    record: &UsageRecord,
) {
    let key = model_key(&record.provider, &record.final_model);
    let entry = models.entry(key).or_insert_with(|| ModelCounter {
        provider: record.provider.clone(),
        model: record.final_model.clone(),
        totals: Counter::default(),
    });
    entry.totals.apply_record(record);
}

fn write_stats_document(path: &Path, document: &StatsDocument) -> Result<(), StatsWriteError> {
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| StatsWriteError::new(path.to_path_buf(), error.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| StatsWriteError::new(path.to_path_buf(), error.to_string()))?;
    }

    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut file = fs::File::create(&tmp)
            .map_err(|error| StatsWriteError::new(path.to_path_buf(), error.to_string()))?;
        file.write_all(&bytes)
            .map_err(|error| StatsWriteError::new(path.to_path_buf(), error.to_string()))?;
        file.flush()
            .map_err(|error| StatsWriteError::new(path.to_path_buf(), error.to_string()))?;
        let _ = file.sync_all();
    }

    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(StatsWriteError::new(path.to_path_buf(), error.to_string()));
    }

    Ok(())
}

fn rename_corrupt_file(path: &Path) {
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let corrupt = path.with_extension(format!("corrupt-{stamp}.json"));
    let _ = fs::rename(path, corrupt);
}

fn model_key(provider: &str, model: &str) -> String {
    format!("{provider}\u{001f}{model}")
}

fn day_key(at: DateTime<Local>) -> String {
    at.format("%Y-%m-%d").to_string()
}

fn quarter_hour_label(at: DateTime<Local>) -> String {
    let minute = (at.minute() / 15) * 15;
    format!("{} {:02}:{:02}", at.format("%Y-%m-%d"), at.hour(), minute)
}

fn range_includes_day(range: UsageRange, today: NaiveDate, day: &str) -> bool {
    match range {
        UsageRange::Live => false,
        UsageRange::All => true,
        UsageRange::Days7 => within_last_days(today, day, 7),
        UsageRange::Days30 => within_last_days(today, day, 30),
    }
}

fn within_last_days(today: NaiveDate, day: &str, window_days: i64) -> bool {
    let Ok(parsed) = NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
        return false;
    };
    let cutoff = today - Duration::days(window_days - 1);
    parsed >= cutoff && parsed <= today
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn record_success_aggregates_tokens_and_zero_failures() {
        let store = UsageStatsStore::in_memory();
        store
            .record(UsageRecord::success("nvidia", "nvidia/a", 120, 30, 0, true))
            .unwrap();

        let snapshot = store.snapshot(UsageRange::Live);
        assert_eq!(snapshot.totals.requests, 1);
        assert_eq!(snapshot.totals.input_tokens, 120);
        assert_eq!(snapshot.totals.output_tokens, 30);
        assert_eq!(snapshot.totals.total_tokens, 150);
        assert_eq!(snapshot.totals.failed_requests, 0);
        assert_eq!(snapshot.totals.retry_count, 0);
    }

    #[test]
    fn final_failure_counts_once_and_keeps_extra_attempts_as_retries() {
        let store = UsageStatsStore::in_memory();
        store
            .record(UsageRecord::failure("grok", "grok-4.5", 2, false))
            .unwrap();

        let totals = store.snapshot(UsageRange::Live).totals;
        assert_eq!(totals.requests, 1);
        assert_eq!(totals.failed_requests, 1);
        assert_eq!(totals.retry_count, 2);
    }

    #[test]
    fn persisted_daily_data_round_trips_and_range_merge_includes_live_data() {
        let dir = unique_test_dir("usage-stats-roundtrip");
        let path = dir.join("usage-stats.json");
        let first = UsageStatsStore::from_path(path.clone()).unwrap();
        first.record_at(
            UsageRecord::success("grok", "grok-4.5", 100, 50, 1, true),
            day("2026-08-08"),
        )
        .unwrap();
        drop(first);

        let second = UsageStatsStore::from_path(path).unwrap();
        second
            .record(UsageRecord::success("nvidia", "nvidia/a", 20, 10, 0, true))
            .unwrap();
        let totals = second.snapshot(UsageRange::All).totals;
        assert_eq!(totals.requests, 2);
        assert_eq!(totals.total_tokens, 180);
        remove_test_dir(dir);
    }

    #[test]
    fn empty_snapshot_defaults_success_rate_to_one() {
        let store = UsageStatsStore::in_memory();

        let snapshot = store.snapshot(UsageRange::All);
        assert_eq!(snapshot.totals.requests, 0);
        assert_eq!(snapshot.totals.total_tokens, 0);
        assert_eq!(snapshot.totals.success_rate, 1.0);
        assert!(snapshot.trend.is_empty());
        assert!(snapshot.models.is_empty());
        assert!(snapshot.providers.is_empty());
    }

    #[test]
    fn corrupt_history_recovers_to_empty_and_keeps_evidence_file() {
        let dir = unique_test_dir("usage-stats-corrupt");
        let path = dir.join("usage-stats.json");
        std::fs::write(&path, b"{ invalid json").unwrap();

        let store = UsageStatsStore::from_path(path).unwrap();
        let snapshot = store.snapshot(UsageRange::All);
        assert!(snapshot.history_recovered);
        assert!(snapshot.history_writable);
        assert_eq!(snapshot.totals.requests, 0);

        let evidence_kept = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .any(|name| name.starts_with("usage-stats.corrupt-") && name.ends_with(".json"));
        assert!(evidence_kept);

        remove_test_dir(dir);
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "claude-launcher-{prefix}-{}-{nonce}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn remove_test_dir(dir: PathBuf) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn day(value: &str) -> DateTime<Local> {
        let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap();
        let noon = date.and_hms_opt(12, 0, 0).unwrap();
        local_datetime(noon)
    }

    fn local_datetime(value: NaiveDateTime) -> DateTime<Local> {
        match Local.from_local_datetime(&value) {
            LocalResult::Single(datetime) => datetime,
            LocalResult::Ambiguous(first, _) => first,
            LocalResult::None => Local.from_utc_datetime(&value),
        }
    }
}
