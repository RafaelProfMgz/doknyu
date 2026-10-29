//! CPU/memory stats computation and a bounded sample history.

use std::collections::VecDeque;

use crate::model::{RawStats, StatsSample};

/// Number of samples kept for the charts (1 per second).
const HISTORY_CAPACITY: usize = 60;

/// Converts a raw Docker stats reading into a displayable sample.
pub fn sample_from_raw(raw: &RawStats) -> StatsSample {
    StatsSample {
        cpu_percent: cpu_percent(raw),
        mem_used: raw.mem_usage.saturating_sub(raw.mem_inactive_file),
        mem_limit: raw.mem_limit,
        net_rx: raw.net_rx,
        net_tx: raw.net_tx,
        blk_read: raw.blk_read,
        blk_write: raw.blk_write,
    }
}

/// CPU usage in percent; 0.0 for the first sample, stopped containers or
/// counters that went backwards (never NaN or infinite).
fn cpu_percent(raw: &RawStats) -> f64 {
    if raw.precpu_total == 0 || raw.presystem == 0 {
        return 0.0;
    }
    let (Some(cpu_delta), Some(sys_delta)) = (
        raw.cpu_total.checked_sub(raw.precpu_total),
        raw.system.checked_sub(raw.presystem),
    ) else {
        return 0.0;
    };
    if sys_delta == 0 {
        return 0.0;
    }
    cpu_delta as f64 / sys_delta as f64 * f64::from(raw.online_cpus) * 100.0
}

/// Rolling window of the latest samples.
#[derive(Debug, Clone, Default)]
pub struct StatsHistory {
    samples: VecDeque<StatsSample>,
}

impl StatsHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, s: StatsSample) {
        if self.samples.len() == HISTORY_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(s);
    }

    pub fn latest(&self) -> Option<&StatsSample> {
        self.samples.back()
    }

    pub fn cpu_series(&self) -> Vec<f64> {
        self.samples.iter().map(|s| s.cpu_percent).collect()
    }

    pub fn mem_series(&self) -> Vec<f64> {
        self.samples.iter().map(|s| s.mem_used as f64).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> RawStats {
        RawStats {
            cpu_total: 400,
            precpu_total: 200,
            system: 2000,
            presystem: 1000,
            online_cpus: 4,
            mem_usage: 500,
            mem_inactive_file: 100,
            mem_limit: 1000,
            net_rx: 1,
            net_tx: 2,
            blk_read: 3,
            blk_write: 4,
        }
    }

    fn sample(cpu: f64) -> StatsSample {
        StatsSample {
            cpu_percent: cpu,
            mem_used: cpu as u64,
            mem_limit: 0,
            net_rx: 0,
            net_tx: 0,
            blk_read: 0,
            blk_write: 0,
        }
    }

    #[test]
    fn cpu_percent_basic() {
        assert_eq!(sample_from_raw(&raw()).cpu_percent, 80.0);
    }

    #[test]
    fn cpu_zero_when_system_delta_zero() {
        let r = RawStats {
            presystem: 2000,
            ..raw()
        };
        assert_eq!(sample_from_raw(&r).cpu_percent, 0.0);
    }

    #[test]
    fn cpu_zero_on_first_sample() {
        let r = RawStats {
            precpu_total: 0,
            presystem: 0,
            ..raw()
        };
        assert_eq!(sample_from_raw(&r).cpu_percent, 0.0);
    }

    #[test]
    fn cpu_zero_when_counters_go_backwards() {
        let r = RawStats {
            cpu_total: 100,
            ..raw()
        };
        assert_eq!(sample_from_raw(&r).cpu_percent, 0.0);
    }

    #[test]
    fn mem_excludes_inactive_file() {
        assert_eq!(sample_from_raw(&raw()).mem_used, 400);
        let r = RawStats {
            mem_inactive_file: 900,
            ..raw()
        };
        assert_eq!(sample_from_raw(&r).mem_used, 0);
    }

    #[test]
    fn other_fields_are_copied() {
        let s = sample_from_raw(&raw());
        assert_eq!(
            (s.mem_limit, s.net_rx, s.net_tx, s.blk_read, s.blk_write),
            (1000, 1, 2, 3, 4)
        );
    }

    #[test]
    fn history_caps_at_60() {
        let mut h = StatsHistory::new();
        for i in 0..61 {
            h.push(sample(i as f64));
        }
        let cpu = h.cpu_series();
        assert_eq!(cpu.len(), 60);
        assert_eq!(cpu[0], 1.0);
        assert_eq!(h.latest().unwrap().cpu_percent, 60.0);
        assert_eq!(h.mem_series()[0], 1.0);
    }
}
