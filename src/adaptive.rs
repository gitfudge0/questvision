use crate::quality::StreamSettings;
use std::time::{Duration, Instant};

pub const TELEMETRY_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptationMode {
    #[default]
    Adaptive,
    Fixed,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ControlMessage {
    BrowserStats(BrowserTelemetry),
    SetMode(AdaptationMode),
    SetCeiling(StreamSettings),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum HostMessage {
    Status(ControllerUpdate),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BrowserTelemetry {
    pub version: u8,
    pub sequence: u64,
    pub sampled_at_ms: u64,
    pub received_bitrate_bps: Option<u64>,
    pub packets_lost: Option<i64>,
    pub jitter_ms: Option<f64>,
    pub frames_decoded: Option<u64>,
    pub frames_dropped: Option<u64>,
    pub rtt_ms: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct HostMetrics {
    pub observed_at: Instant,
    pub capture_receive_interval: Option<Duration>,
    pub resize_time: Duration,
    pub color_convert_time: Duration,
    pub encode_time: Duration,
    pub queue_wait: Option<Duration>,
    pub queue_saturated: bool,
    pub cadence_missed: bool,
    pub frame_age_since_capture_delivery: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureReason {
    Stable,
    NetworkPressure,
    HostPressure,
    CombinedPressure,
    BrowserFeedbackUnavailable,
    HostFeedbackUnavailable,
    AtLowestTier,
    ApplyFailed,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ControllerUpdate {
    pub mode: AdaptationMode,
    pub ceiling: StreamSettings,
    pub effective: StreamSettings,
    pub reason: PressureReason,
    pub browser_feedback_available: bool,
    pub host_feedback_available: bool,
    pub error: Option<String>,
}

/// Starting values are provisional, not empirically tuned thresholds.
#[derive(Clone, Copy, Debug)]
pub struct ControllerPolicy {
    pub provisional: bool,
    pub network_pressure_persistence: Duration,
    pub host_pressure_persistence: Duration,
    pub recovery_persistence: Duration,
    pub transition_interval: Duration,
    pub browser_freshness: Duration,
    pub host_freshness: Duration,
    pub jitter_pressure_ms: f64,
    pub rtt_pressure_ms: f64,
    pub lost_packets_per_second: f64,
    pub dropped_frame_ratio: f64,
    pub received_bitrate_ratio: f64,
    pub decoded_fps_ratio: f64,
    pub processing_frame_budget_ratio: f64,
    pub queue_frame_budget_ratio: f64,
    pub frame_age_budget_ratio: f64,
}

pub const PROVISIONAL_POLICY: ControllerPolicy = ControllerPolicy {
    provisional: true,
    network_pressure_persistence: Duration::from_secs(3),
    host_pressure_persistence: Duration::from_secs(3),
    recovery_persistence: Duration::from_secs(12),
    transition_interval: Duration::from_secs(3),
    browser_freshness: Duration::from_secs(4),
    host_freshness: Duration::from_secs(3),
    jitter_pressure_ms: 35.0,
    rtt_pressure_ms: 180.0,
    lost_packets_per_second: 3.0,
    dropped_frame_ratio: 0.08,
    received_bitrate_ratio: 0.55,
    decoded_fps_ratio: 0.70,
    processing_frame_budget_ratio: 0.90,
    queue_frame_budget_ratio: 1.0,
    frame_age_budget_ratio: 2.0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Condition {
    Unknown,
    Healthy,
    Pressure,
}

#[derive(Debug)]
struct PressureTrack {
    condition: Condition,
    complete: bool,
    observed_at: Option<Instant>,
    pressure_since: Option<Instant>,
    required_tier: usize,
}

impl PressureTrack {
    fn new() -> Self {
        Self {
            condition: Condition::Unknown,
            complete: false,
            observed_at: None,
            pressure_since: None,
            required_tier: 0,
        }
    }

    fn fresh(&self, now: Instant, freshness: Duration) -> bool {
        self.observed_at.is_some_and(|at| {
            now.checked_duration_since(at)
                .is_some_and(|age| age <= freshness)
        })
    }

    fn observe(&mut self, condition: Condition, complete: bool, at: Instant, freshness: Duration) {
        let continuous = self.fresh(at, freshness);
        if condition != Condition::Pressure {
            self.pressure_since = None;
        } else if !continuous || self.condition != Condition::Pressure {
            self.pressure_since = Some(at);
        }
        self.condition = condition;
        self.complete = complete;
        self.observed_at = Some(at);
    }

    fn persistent(&self, now: Instant, freshness: Duration, persistence: Duration) -> bool {
        self.fresh(now, freshness)
            && self.condition == Condition::Pressure
            && self
                .pressure_since
                .is_some_and(|since| now.saturating_duration_since(since) >= persistence)
    }
}

pub struct AdaptiveController {
    mode: AdaptationMode,
    ceiling: StreamSettings,
    tiers: Vec<StreamSettings>,
    tier: usize,
    effective: StreamSettings,
    desired_target: StreamSettings,
    pending: Option<StreamSettings>,
    browser: PressureTrack,
    host: PressureTrack,
    last_browser: Option<BrowserTelemetry>,
    healthy_since: Option<Instant>,
    last_transition: Option<Instant>,
    error: Option<String>,
    policy: ControllerPolicy,
}

impl AdaptiveController {
    pub fn new(ceiling: StreamSettings, mode: AdaptationMode) -> Self {
        let policy = PROVISIONAL_POLICY;
        let tiers = ceiling.tier_candidates();
        tracing::info!(
            ?policy,
            ?tiers,
            ?mode,
            "adaptive controller policy (provisional)"
        );
        Self {
            mode,
            ceiling,
            tiers,
            tier: 0,
            effective: ceiling,
            desired_target: ceiling,
            pending: None,
            browser: PressureTrack::new(),
            host: PressureTrack::new(),
            last_browser: None,
            healthy_since: None,
            last_transition: None,
            error: None,
            policy,
        }
    }

    pub fn observe_browser(
        &mut self,
        sample: BrowserTelemetry,
        received_at: Instant,
    ) -> ControllerUpdate {
        let now = Instant::now();
        if !self.valid_browser(&sample, received_at, now) {
            tracing::debug!(
                sequence = sample.sequence,
                "ignored invalid, stale, or reordered browser telemetry"
            );
            return self.current();
        }
        let continuous = self
            .browser
            .fresh(received_at, self.policy.browser_freshness);
        if !continuous {
            self.healthy_since = None;
        }
        let (condition, complete) = self.classify_browser(&sample, continuous);
        tracing::debug!(
            ?sample,
            ?received_at,
            ?condition,
            complete,
            "browser controller input"
        );
        self.browser.observe(
            condition,
            complete,
            received_at,
            self.policy.browser_freshness,
        );
        self.last_browser = Some(sample);
        self.evaluate(now);
        self.current()
    }

    pub fn observe_host(&mut self, sample: HostMetrics) -> ControllerUpdate {
        let now = Instant::now();
        if !fresh_arrival(sample.observed_at, now, self.policy.host_freshness)
            || self
                .host
                .observed_at
                .is_some_and(|last| sample.observed_at <= last)
        {
            tracing::debug!("ignored stale or reordered host metrics");
            return self.current();
        }
        let budget = 1.0 / f64::from(self.effective.fps.max(1));
        if !self
            .host
            .fresh(sample.observed_at, self.policy.host_freshness)
        {
            self.healthy_since = None;
        }
        let processing = sample.resize_time.as_secs_f64()
            + sample.color_convert_time.as_secs_f64()
            + sample.encode_time.as_secs_f64();
        let pressure = processing >= budget * self.policy.processing_frame_budget_ratio
            || sample.queue_wait.is_some_and(|wait| {
                wait.as_secs_f64() >= budget * self.policy.queue_frame_budget_ratio
            })
            || sample.frame_age_since_capture_delivery.as_secs_f64()
                >= budget * self.policy.frame_age_budget_ratio
            || sample.queue_saturated
            || sample.cadence_missed;
        let complete = sample.queue_wait.is_some();
        let condition = if pressure {
            Condition::Pressure
        } else if complete {
            Condition::Healthy
        } else {
            Condition::Unknown
        };
        tracing::debug!(
            ?sample,
            ?condition,
            complete,
            "host controller input; frame age is since capture API delivery"
        );
        self.host.observe(
            condition,
            complete,
            sample.observed_at,
            self.policy.host_freshness,
        );
        self.evaluate(now);
        self.current()
    }

    pub fn set_mode(&mut self, mode: AdaptationMode) -> ControllerUpdate {
        if self.mode == mode && self.error.is_none() {
            return self.current();
        }
        self.mode = mode;
        self.error = None;
        self.pending = None;
        self.reset_evidence();
        if mode == AdaptationMode::Fixed {
            self.select(0, Instant::now(), PressureReason::Stable);
        } else {
            self.select(self.tier, Instant::now(), PressureReason::Stable);
        }
        self.current()
    }

    pub fn set_ceiling(&mut self, ceiling: StreamSettings) -> ControllerUpdate {
        if !ceiling.is_valid() {
            tracing::debug!(?ceiling, "ignored invalid ceiling");
            return self.current();
        }
        if self.ceiling == ceiling && self.error.is_none() {
            return self.current();
        }
        let previous = self.effective;
        self.ceiling = ceiling;
        self.tiers = ceiling.tier_candidates();
        self.tier = self.index_for(previous);
        self.error = None;
        self.pending = None;
        self.reset_evidence();
        // Raising a ceiling does not bypass the slower adaptive recovery interval.
        let tier = if self.mode == AdaptationMode::Fixed {
            0
        } else {
            self.tiers
                .iter()
                .position(|candidate| {
                    candidate.preset.rank() <= previous.preset.rank()
                        && candidate.fps <= previous.fps
                        && candidate.bitrate_mbps <= previous.bitrate_mbps
                })
                .unwrap_or(self.tiers.len() - 1)
        };
        self.select(tier, Instant::now(), PressureReason::Stable);
        self.browser.required_tier = tier;
        self.host.required_tier = tier;
        self.current()
    }

    /// The single pending policy proposal; effective status stays host-confirmed.
    pub fn desired_settings(&self) -> Option<StreamSettings> {
        self.pending
    }

    pub fn report_apply_success(&mut self, settings: StreamSettings) -> ControllerUpdate {
        if !settings.is_valid() {
            tracing::warn!(
                ?settings,
                "ignored invalid applied settings acknowledgement"
            );
            return self.current();
        }
        let now = Instant::now();
        self.effective = settings;
        // An old apply can finish after the latest target was already effective
        // and had no pending work. Keep that target independently of pending.
        self.reconcile_pending();
        self.tier = self.index_for(settings);
        self.last_transition = Some(now);
        self.error = None;
        self.reset_evidence();
        tracing::info!(?settings, ?now, "adaptive settings confirmed by host");
        self.current()
    }

    pub fn report_apply_failure(&mut self, message: String) -> ControllerUpdate {
        self.pending = None;
        self.error = Some(message);
        self.reset_evidence();
        tracing::warn!(effective = ?self.effective, error = ?self.error, "adaptive policy apply failed; holding previous settings");
        self.current()
    }

    pub fn mark_browser_unavailable(&mut self) -> ControllerUpdate {
        // Retain arrival and browser sequence/sample ordering for this session,
        // while removing all browser pressure and shared recovery evidence.
        let observed_at = self.browser.observed_at;
        self.browser = PressureTrack::new();
        self.browser.observed_at = observed_at;
        self.healthy_since = None;
        tracing::info!("browser feedback unavailable; preserving host adaptation");
        self.current()
    }

    pub fn current(&self) -> ControllerUpdate {
        let now = Instant::now();
        let browser_fresh = self.browser.fresh(now, self.policy.browser_freshness);
        let host_fresh = self.host.fresh(now, self.policy.host_freshness);
        let browser_available = browser_fresh && self.browser.complete;
        let host_available = host_fresh && self.host.complete;
        let network_pressure = browser_fresh && self.browser.condition == Condition::Pressure;
        let host_pressure = host_fresh && self.host.condition == Condition::Pressure;
        let reason = if self.error.is_some() {
            PressureReason::ApplyFailed
        } else if (network_pressure || host_pressure) && self.tier == self.tiers.len() - 1 {
            PressureReason::AtLowestTier
        } else if network_pressure && host_pressure {
            PressureReason::CombinedPressure
        } else if network_pressure {
            PressureReason::NetworkPressure
        } else if host_pressure {
            PressureReason::HostPressure
        } else if !browser_available {
            PressureReason::BrowserFeedbackUnavailable
        } else if !host_available {
            PressureReason::HostFeedbackUnavailable
        } else {
            PressureReason::Stable
        };
        ControllerUpdate {
            mode: self.mode,
            ceiling: self.ceiling,
            effective: self.effective,
            reason,
            browser_feedback_available: browser_available,
            host_feedback_available: host_available,
            error: self.error.clone(),
        }
    }

    fn valid_browser(&self, sample: &BrowserTelemetry, received_at: Instant, now: Instant) -> bool {
        sample.version == TELEMETRY_VERSION
            && [sample.jitter_ms, sample.rtt_ms]
                .into_iter()
                .flatten()
                .all(|value| value.is_finite() && value >= 0.0)
            && fresh_arrival(received_at, now, self.policy.browser_freshness)
            && self
                .browser
                .observed_at
                .is_none_or(|last| received_at > last)
            && self.last_browser.as_ref().is_none_or(|last| {
                sample.sequence > last.sequence && sample.sampled_at_ms > last.sampled_at_ms
            })
    }

    fn classify_browser(&self, sample: &BrowserTelemetry, continuous: bool) -> (Condition, bool) {
        let previous = self.last_browser.as_ref().filter(|_| continuous);
        let seconds =
            previous.map(|last| (sample.sampled_at_ms - last.sampled_at_ms) as f64 / 1000.0);
        // Signed packet-loss totals are legal in WebRTC. Decreasing totals and
        // reset/missing counters supply a new baseline, never a healthy delta.
        let loss = previous
            .and_then(|last| sample.packets_lost?.checked_sub(last.packets_lost?))
            .filter(|delta| *delta >= 0)
            .zip(seconds)
            .map(|(delta, seconds)| delta as f64 / seconds);
        let decoded =
            previous.and_then(|last| sample.frames_decoded?.checked_sub(last.frames_decoded?));
        let dropped =
            previous.and_then(|last| sample.frames_dropped?.checked_sub(last.frames_dropped?));
        let dropped_ratio = decoded.zip(dropped).and_then(|(decoded, dropped)| {
            let total = u128::from(decoded) + u128::from(dropped);
            (total > 0).then(|| dropped as f64 / total as f64)
        });
        let decode_rate = decoded
            .zip(seconds)
            .map(|(frames, seconds)| frames as f64 / seconds);
        let low_receive =
            sample
                .received_bitrate_bps
                .zip(decode_rate)
                .is_some_and(|(bitrate, fps)| {
                    (bitrate as f64)
                        < f64::from(self.effective.bitrate_mbps)
                            * 1_000_000.0
                            * self.policy.received_bitrate_ratio
                        && fps < f64::from(self.effective.fps) * self.policy.decoded_fps_ratio
                });
        let pressure = sample
            .jitter_ms
            .is_some_and(|jitter| jitter >= self.policy.jitter_pressure_ms)
            || sample
                .rtt_ms
                .is_some_and(|rtt| rtt >= self.policy.rtt_pressure_ms)
            || loss.is_some_and(|rate| rate >= self.policy.lost_packets_per_second)
            || dropped_ratio.is_some_and(|ratio| ratio >= self.policy.dropped_frame_ratio)
            || low_receive;
        let complete = sample.received_bitrate_bps.is_some()
            && sample.jitter_ms.is_some()
            && sample.rtt_ms.is_some()
            && loss.is_some()
            && dropped_ratio.is_some();
        let condition = if pressure {
            Condition::Pressure
        } else if complete {
            Condition::Healthy
        } else {
            Condition::Unknown
        };
        (condition, complete)
    }

    fn evaluate(&mut self, now: Instant) {
        if self.mode == AdaptationMode::Fixed || self.error.is_some() || self.pending.is_some() {
            self.healthy_since = None;
            return;
        }
        let network_down = self.browser.persistent(
            now,
            self.policy.browser_freshness,
            self.policy.network_pressure_persistence,
        );
        let host_down = self.host.persistent(
            now,
            self.policy.host_freshness,
            self.policy.host_pressure_persistence,
        );
        let rate_ready = self.last_transition.is_none_or(|last| {
            now.saturating_duration_since(last) >= self.policy.transition_interval
        });
        if rate_ready && (network_down || host_down) && self.tier + 1 < self.tiers.len() {
            let next = self.tier + 1;
            if network_down {
                self.browser.required_tier = next;
                self.browser.pressure_since = Some(now);
            }
            if host_down {
                self.host.required_tier = next;
                self.host.pressure_since = Some(now);
            }
            let reason = match (network_down, host_down) {
                (true, true) => PressureReason::CombinedPressure,
                (true, false) => PressureReason::NetworkPressure,
                _ => PressureReason::HostPressure,
            };
            self.select(
                self.browser.required_tier.max(self.host.required_tier),
                now,
                reason,
            );
            self.healthy_since = None;
            return;
        }
        let healthy = self.browser.fresh(now, self.policy.browser_freshness)
            && self.host.fresh(now, self.policy.host_freshness)
            && self.browser.condition == Condition::Healthy
            && self.host.condition == Condition::Healthy;
        if !healthy {
            self.healthy_since = None;
        } else {
            let since = *self.healthy_since.get_or_insert(now);
            if rate_ready
                && self.tier > 0
                && now.saturating_duration_since(since) >= self.policy.recovery_persistence
            {
                let next = self.tier - 1;
                self.browser.required_tier = self.browser.required_tier.min(next);
                self.host.required_tier = self.host.required_tier.min(next);
                self.select(next, now, PressureReason::Stable);
                self.healthy_since = Some(now);
            }
        }
    }

    fn select(&mut self, tier: usize, now: Instant, reason: PressureReason) {
        let next = self.tiers[tier];
        self.desired_target = next;
        if self.effective != next {
            tracing::info!(previous = ?self.effective, desired = ?next, ?reason, ?now, tier, "adaptive tier proposal");
            self.pending = Some(next);
        } else {
            self.tier = tier;
            self.pending = None;
        }
    }

    fn reconcile_pending(&mut self) {
        let target = if self.mode == AdaptationMode::Fixed {
            self.ceiling
        } else {
            // Recheck the latest target against the active ceiling's ladder.
            self.tiers[self.index_for(self.desired_target)]
        };
        self.desired_target = target;
        self.pending = (target != self.effective).then_some(target);
    }

    fn index_for(&self, settings: StreamSettings) -> usize {
        self.tiers
            .iter()
            .position(|tier| *tier == settings)
            .unwrap_or_else(|| {
                self.tiers
                    .iter()
                    .position(|tier| {
                        tier.preset.rank() <= settings.preset.rank()
                            && tier.fps <= settings.fps
                            && tier.bitrate_mbps <= settings.bitrate_mbps
                    })
                    .unwrap_or(self.tiers.len() - 1)
            })
    }

    fn reset_evidence(&mut self) {
        let browser_at = self.browser.observed_at;
        let host_at = self.host.observed_at;
        self.browser = PressureTrack::new();
        self.host = PressureTrack::new();
        // Mode/tier changes never start a new browser session or reset ordering.
        self.browser.observed_at = browser_at;
        self.host.observed_at = host_at;
        self.browser.required_tier = self.tier;
        self.host.required_tier = self.tier;
        self.healthy_since = None;
    }
}

fn fresh_arrival(at: Instant, now: Instant, freshness: Duration) -> bool {
    now.checked_duration_since(at)
        .is_some_and(|age| age <= freshness)
}
