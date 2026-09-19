//! Footer presentation of authoritative live monitor snapshots.

#[cfg(test)]
mod tests;

use codex_app_server_protocol::ThreadMonitor;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::ChatWidget;

impl ChatWidget {
    pub(crate) fn set_monitor_status(&mut self, monitors: &[ThreadMonitor]) {
        self.bottom_pane
            .set_monitor_status_indicator(monitor_status_line(monitors));
    }
}

fn monitor_status_line(monitors: &[ThreadMonitor]) -> Option<Line<'static>> {
    let mut intervals = monitors
        .iter()
        .filter_map(|monitor| monitor.interval_minutes)
        .collect::<Vec<_>>();
    intervals.sort_by(f64::total_cmp);
    let summary_count = intervals.len();
    let realtime_count = monitors.len() - summary_count;
    let mut line = Line::default();
    if summary_count > 0 {
        intervals.dedup();
        let cadence = if intervals.len() == 1 {
            format!("{}m", intervals[0])
        } else {
            format!("{}–{}m", intervals[0], intervals[intervals.len() - 1])
        };
        line.push_span(format!("monitor ×{summary_count} ({cadence})").cyan());
    }
    if realtime_count > 0 {
        if summary_count > 0 {
            line.push_span(" · ".dim());
        }
        line.push_span(format!("monitor_realtime ×{realtime_count}").magenta());
    }
    (!monitors.is_empty()).then_some(line)
}
