use std::sync::atomic::AtomicUsize;

use tracing::{debug, info};

pub static USAGE: AtomicUsize = AtomicUsize::new(0);
pub static PEAK_USAGE: AtomicUsize = AtomicUsize::new(0);
pub static TOTAL_WAIT: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn increment_usage() {
    let current = USAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let peak = PEAK_USAGE.load(std::sync::atomic::Ordering::Relaxed);
    if current > peak {
        debug!("Peak usage: {}", current);
        PEAK_USAGE.store(current, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn decrement_usage() -> usize {
    USAGE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn log_usage() {
    info!(
        "Peak usage: {}",
        PEAK_USAGE.load(std::sync::atomic::Ordering::Relaxed)
    );
    info!(
        "Total wait time: {}ms",
        TOTAL_WAIT.load(std::sync::atomic::Ordering::Relaxed)
    );
}
