use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn composer_with_monitors() -> ChatComposer {
    let (mut composer, _rx) = super::tests::new_test_composer();
    composer.set_status_line_enabled(true);
    composer.set_status_line(Some(Line::from("gpt-6-astra low · ~ · Main [default]")));
    composer
        .set_monitor_status_indicator(Some(Line::from("monitor ×1 (60m) · monitor_realtime ×1")));
    composer.set_goal_status_indicator(Some(GoalStatusIndicator::Complete {
        usage: Some("18m".to_string()),
    }));
    composer
}

fn render_composer(composer: &ChatComposer, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 5)).expect("terminal");
    terminal
        .draw(|frame| composer.render(frame.area(), frame.buffer_mut()))
        .expect("render composer");
    terminal.backend().to_string()
}

#[test]
fn monitor_footer_coexists_with_completed_goal_and_clears() {
    let mut composer = composer_with_monitors();
    let rendered = render_composer(&composer, 140);
    let footer = rendered
        .lines()
        .find(|line| line.contains("monitor ×1"))
        .expect("monitor status is visible");
    assert!(footer.contains("monitor ×1 (60m)"));
    assert!(footer.contains("monitor_realtime ×1"));
    assert!(footer.contains("Goal achieved (18m)"));
    assert!(footer.contains("gpt-6-astra"));
    insta::assert_snapshot!("monitor_footer_mixed_wide", rendered);

    composer.set_monitor_status_indicator(None);
    let cleared = render_composer(&composer, 140);
    assert!(!cleared.contains("monitor"));
    assert!(cleared.contains("Goal achieved (18m)"));
}

#[test]
fn monitor_footer_remains_visible_on_narrow_terminal() {
    let rendered = render_composer(&composer_with_monitors(), 60);
    assert!(rendered.contains("monitor"), "{rendered}");
    insta::assert_snapshot!("monitor_footer_mixed_narrow", rendered);
}
