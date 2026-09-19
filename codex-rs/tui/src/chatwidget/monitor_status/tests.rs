use super::*;
use pretty_assertions::assert_eq;

fn monitor(interval_minutes: Option<f64>) -> ThreadMonitor {
    ThreadMonitor {
        id: "test".to_string(),
        description: "test monitor".to_string(),
        interval_minutes,
    }
}

#[test]
fn monitor_footer_distinguishes_summary_and_realtime() {
    let line = monitor_status_line(&[monitor(Some(60.0)), monitor(None), monitor(Some(60.0))])
        .expect("live monitors have a footer");
    assert_eq!(line.to_string(), "monitor ×2 (60m) · monitor_realtime ×1");
}

#[test]
fn monitor_footer_reports_interval_range_and_fractional_minutes() {
    let line = monitor_status_line(&[monitor(Some(60.0)), monitor(Some(0.5))])
        .expect("live monitors have a footer");
    assert_eq!(line.to_string(), "monitor ×2 (0.5–60m)");
}

#[test]
fn monitor_footer_disappears_when_snapshot_is_empty() {
    assert_eq!(monitor_status_line(&[]), None);
}
