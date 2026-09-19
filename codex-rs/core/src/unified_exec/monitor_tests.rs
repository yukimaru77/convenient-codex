use super::*;
use pretty_assertions::assert_eq;

#[test]
fn summary_keeps_head_tail_and_resets_without_losing_partial_line() {
    let mut buffer = SummaryBuffer::default();
    for i in 0..30 {
        buffer.extend(format!("line {i}\n").as_bytes());
    }
    buffer.extend(b"next");
    let mut expected = (0..3).map(|i| format!("line {i}")).collect::<Vec<_>>();
    expected.push("... (7 lines omitted) ...".into());
    expected.extend((10..30).map(|i| format!("line {i}")));
    assert_eq!(buffer.take_summary(), Some(expected.join("\n")));
    assert_eq!(buffer.take_summary(), None);
    buffer.extend(b" line\n");
    assert_eq!(buffer.take_summary(), Some("next line".into()));
}

#[test]
fn summary_preserves_short_batches_without_overlap() {
    for count in [1, 3, 4, 20, 23] {
        let text = (0..count)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut buffer = SummaryBuffer::default();
        buffer.extend(format!("{text}\n").as_bytes());
        assert_eq!(buffer.take_summary(), Some(text));
    }
}

#[test]
fn summary_bounds_long_lines_and_large_stream_without_losing_tail() {
    let mut buffer = SummaryBuffer::default();
    for _ in 0..6000 {
        buffer.extend("あ".repeat(1000).as_bytes());
        buffer.extend(b"\n");
    }
    buffer.extend(b"last line without newline");
    buffer.finish_line();
    let text = buffer.take_summary().unwrap();
    assert!(text.contains("... (5978 lines omitted) ..."));
    assert!(text.ends_with("last line without newline"));
    assert!(text.len() < 8192);
    assert_eq!(MonitorNotification::summary("label", &text).body, text);
}

#[test]
fn summary_invalid_utf8_cannot_expand_past_notification_limit() {
    let mut buffer = SummaryBuffer::default();
    for _ in 0..30 {
        buffer.extend(&[255; 256]);
        buffer.extend(b"\n");
    }
    buffer.extend(b"tail marker\n");
    let text = buffer.take_summary().unwrap();
    assert!(text.len() < 8192);
    assert!(text.ends_with("tail marker"));
    assert_eq!(MonitorNotification::summary("label", &text).body, text);
}

#[tokio::test]
async fn monitor_context_is_bounded_and_overflow_is_visible() {
    use crate::context::ContextualUserFragment;
    let text = MonitorNotification::new("監視".repeat(100), "あ".repeat(2000)).render();
    assert!(text.len() < 900);
    assert!(text.contains("[truncated]"));
}

#[tokio::test]
async fn long_lines_are_bounded_even_with_a_trailing_newline() {
    let mut buffer = Vec::new();
    let mut lines = Vec::new();
    let mut count = 0;
    let mut deadline = None;
    let data = format!("{}\n", "x".repeat(100_000));
    assert!(!extend_lines(
        &mut buffer,
        data.as_bytes(),
        &mut lines,
        &mut count,
        &mut deadline
    ));
    assert!(buffer.is_empty());
    assert!(lines.iter().all(|line| line.len() <= MAX_LINE_BYTES + 32));
    assert!(lines[0].starts_with("(line truncated)"));
}

#[tokio::test]
async fn registry_tracks_insert_list_and_remove() {
    let manager = MonitorManager::new();
    manager
        .insert(
            "mon_a".to_string(),
            1,
            "watch a".to_string(),
            "cmd a".to_string(),
            MonitorDelivery::Summary {
                interval: Duration::from_secs(3600),
            },
            tokio::spawn(async {}),
        )
        .await;
    manager
        .insert(
            "mon_b".to_string(),
            2,
            "watch b".to_string(),
            "cmd b".to_string(),
            MonitorDelivery::Realtime,
            tokio::spawn(async {}),
        )
        .await;

    assert_eq!(manager.list().await.len(), 2);

    // `remove` returns the process id so the caller can terminate it; a
    // second remove of the same id is a no-op.
    assert_eq!(manager.remove("mon_a").await, Some(1));
    assert_eq!(manager.remove("mon_a").await, None);

    let remaining = manager.list().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "mon_b");
    assert!(matches!(remaining[0].delivery, MonitorDelivery::Realtime));

    manager.abort_all().await;
    assert!(manager.list().await.is_empty());
}

#[tokio::test]
async fn deregister_self_removes_entry_without_aborting_its_task() {
    let manager = MonitorManager::new();
    // A task that runs until aborted, so we can observe whether it survives.
    let task = tokio::spawn(std::future::pending::<()>());
    let handle = task.abort_handle();
    manager
        .insert(
            "mon_x".to_string(),
            7,
            "watch".to_string(),
            "cmd".to_string(),
            MonitorDelivery::Realtime,
            task,
        )
        .await;
    assert_eq!(manager.list().await.len(), 1);

    manager.deregister_self("mon_x").await;
    assert!(manager.list().await.is_empty(), "entry pruned");
    // Unlike `remove`, deregister_self must NOT abort the entry's task: the
    // loop removing itself still has its final exit notice to deliver.
    tokio::task::yield_now().await;
    assert!(
        !handle.is_finished(),
        "deregister_self must not abort the entry's task"
    );

    // Deregistering an absent id is a no-op.
    manager.deregister_self("mon_x").await;
    assert!(manager.list().await.is_empty());

    handle.abort();
}
