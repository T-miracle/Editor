//! Native WASI handles exercise bounded admission, overload recovery, UTF-8 framing and retirement.
use super::*;

/// Wait for asynchronous publication, never hiding a missing record behind an unbounded wait.
fn records_through(logs: &RuntimeLogs, count: usize) -> Vec<crate::LogRecord> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let records = logs.records("guest");
        if records.len() >= count || std::time::Instant::now() >= deadline {
            return records;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Real WASI handles share framing, preserve split UTF-8 and flush the final line after retirement.
#[test]
fn wasi_output_adapter_keeps_multibyte_and_retirement_output() {
    let logs = RuntimeLogs::default();
    let output = capture(logs.clone(), "guest", LogLevel::Info, "wasi/stdout").unwrap();
    let mut first = output.p2_stream();
    let mut second = output.p2_stream();
    assert_eq!(first.check_write().unwrap(), WRITE_LIMIT);
    first.write(Bytes::from_static(&[0xe7])).unwrap();
    second.write(Bytes::from_static(&[0xac, 0xac])).unwrap();
    first.write("一行\n尾".as_bytes().to_vec().into()).unwrap();
    second.write("行".as_bytes().to_vec().into()).unwrap();
    drop(first);
    drop(second);
    drop(output);
    let records = records_through(&logs, 2);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].message, "第一行");
    assert_eq!(records[1].message, "尾行");
    assert!(
        records
            .iter()
            .all(|record| { record.level == LogLevel::Info && record.source == "wasi/stdout" })
    );
}

/// An oversized host write fails before copying or walking its bytes and retains a clear warning.
#[test]
fn oversized_wasi_write_is_rejected_with_a_single_truncation_notice() {
    let logs = RuntimeLogs::default();
    let output = capture(logs.clone(), "guest", LogLevel::Info, "wasi/stdout").unwrap();
    let mut stream = output.p2_stream();
    stream.write(Bytes::from_static(b"accepted tail")).unwrap();
    assert_eq!(stream.check_write().unwrap(), WRITE_LIMIT);
    assert!(
        stream
            .write(Bytes::from(vec![b'x'; WRITE_LIMIT + 1]))
            .is_err()
    );
    assert!(matches!(stream.check_write(), Err(StreamError::Closed)));
    assert!(stream.write(Bytes::from_static(b"discarded")).is_err());
    drop(stream);
    drop(output);
    let records = records_through(&logs, 2);
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .any(|record| record.message == "accepted tail")
    );
    let notices: Vec<_> = records
        .iter()
        .filter(|record| record.message.contains("truncated"))
        .collect();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].level, LogLevel::Warning);
}

/// A reader can publish its truncation notice while the Store still owns handles; none can reopen it.
#[test]
fn published_truncation_does_not_reopen_a_retained_wasi_handle() {
    let logs = RuntimeLogs::default();
    let output = capture(logs.clone(), "guest", LogLevel::Info, "wasi/stdout").unwrap();
    let mut stream = output.p2_stream();
    assert!(
        stream
            .write(Bytes::from(vec![b'x'; WRITE_LIMIT + 1]))
            .is_err()
    );
    let records = records_through(&logs, 1);
    assert_eq!(records.len(), 1);
    assert!(records[0].message.contains("truncated"));
    // Only an illegal permit closes admission; reporting it cannot reopen a retained handle.
    assert!(output.0.closed.load(Ordering::Acquire));
    assert!(matches!(stream.check_write(), Err(StreamError::Closed)));
    assert!(
        stream
            .write(Bytes::from_static(b"cannot revive output"))
            .is_err()
    );
    assert_eq!(logs.records("guest").len(), 1);
}

/// Logging congestion discards bounded chunks without failing a legal guest write or losing the EOF notice.
#[test]
fn full_output_queue_discards_without_failing_legal_guest_writes() {
    let logs = RuntimeLogs::default();
    let (output, reader) = output_channel();
    let mut stream = output.p2_stream();
    for _ in 0..QUEUED_CHUNKS {
        assert_eq!(stream.check_write().unwrap(), WRITE_LIMIT);
        stream.write(Bytes::from_static(b"accepted\n")).unwrap();
    }
    for _ in 0..64 {
        stream.write(Bytes::from_static(b"discarded\n")).unwrap();
    }
    assert_eq!(stream.check_write().unwrap(), WRITE_LIMIT);
    assert!(!output.0.closed.load(Ordering::Acquire));
    drop(stream);
    drop(output);
    reader.run(&logs, "guest", LogLevel::Info, "wasi/stdout");
    let records = logs.records("guest");
    assert_eq!(records.len(), QUEUED_CHUNKS + 1);
    let notices: Vec<_> = records
        .iter()
        .filter(|record| record.level == LogLevel::Warning)
        .collect();
    assert_eq!(notices.len(), 1);
    assert!(notices[0].message.contains("Dropped 64"));
    assert!(records.iter().all(|record| record.message != "discarded"));
}

/// After capacity returns, collection resumes without grafting a lost UTF-8 suffix onto an earlier line.
#[test]
fn legal_output_recovers_after_a_dropped_chunk_and_keeps_the_final_tail() {
    let logs = RuntimeLogs::default();
    let (output, reader) = output_channel();
    let mut stream = output.p2_stream();
    for _ in 1..QUEUED_CHUNKS {
        stream.write(Bytes::from_static(b"accepted\n")).unwrap();
    }
    stream
        .write(Bytes::from_static(b"unreliable prefix\xe7"))
        .unwrap();
    // A missing UTF-8 continuation cannot be silently completed by any later admitted chunk.
    stream
        .write(Bytes::from_static(&[0xac, 0xac, b'\n']))
        .unwrap();
    let reader_logs = logs.clone();
    let (finished, done) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        reader.run(&reader_logs, "guest", LogLevel::Info, "wasi/stdout");
        finished.send(()).unwrap();
    });
    // A loss notice may arrive between data records if the scheduler delays this reader's first turn.
    assert!(records_through(&logs, QUEUED_CHUNKS - 1).len() >= QUEUED_CHUNKS - 1);
    stream
        .write(Bytes::from_static(b"recovered line\nfinal \xe5"))
        .unwrap();
    stream.write(Bytes::from_static(&[0xb0, 0xbe])).unwrap();
    assert_eq!(stream.check_write().unwrap(), WRITE_LIMIT);
    drop(stream);
    drop(output);
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.join().unwrap();
    let records = logs.records("guest");
    assert_eq!(
        records
            .iter()
            .filter(|record| record.message == "accepted")
            .count(),
        QUEUED_CHUNKS - 1
    );
    assert!(
        records
            .iter()
            .any(|record| record.message == "recovered line")
    );
    assert!(records.iter().any(|record| record.message == "final 尾"));
    assert!(
        records
            .iter()
            .all(|record| !record.message.contains("unreliable prefix"))
    );
    let notices: Vec<_> = records
        .iter()
        .filter(|record| record.level == LogLevel::Warning)
        .collect();
    assert_eq!(notices.len(), 1);
    assert!(notices[0].message.contains("Dropped 1"));
}

/// An idle live handle receives one coalesced loss warning and can still write successfully afterward.
#[test]
fn idle_output_coalesces_overload_without_waiting_for_retirement() {
    let logs = RuntimeLogs::default();
    let (output, reader) = output_channel();
    let mut stream = output.p2_stream();
    for _ in 0..QUEUED_CHUNKS {
        stream.write(Bytes::from_static(b"accepted\n")).unwrap();
    }
    for _ in 0..256 {
        stream.write(Bytes::from_static(b"discarded\n")).unwrap();
    }
    let reader_logs = logs.clone();
    let (finished, done) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        reader.run(&reader_logs, "guest", LogLevel::Info, "wasi/stdout");
        finished.send(()).unwrap();
    });
    let records = records_through(&logs, QUEUED_CHUNKS + 1);
    let notices: Vec<_> = records
        .iter()
        .filter(|record| record.level == LogLevel::Warning)
        .collect();
    assert_eq!(notices.len(), 1);
    assert!(notices[0].message.contains("Dropped 256"));
    assert_eq!(stream.check_write().unwrap(), WRITE_LIMIT);
    stream
        .write(Bytes::from_static(b"still collecting\n"))
        .unwrap();
    drop(stream);
    drop(output);
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.join().unwrap();
    let records = logs.records("guest");
    assert!(
        records
            .iter()
            .any(|record| record.message == "still collecting")
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.level == LogLevel::Warning)
            .count(),
        1
    );
}

/// One legal newline burst is bounded before any worker publication; retained history still has its cap.
#[test]
fn large_newline_output_and_final_stderr_are_bounded() {
    let logs = RuntimeLogs::default();
    let output = capture(logs.clone(), "guest", LogLevel::Warning, "wasi/stderr").unwrap();
    let mut stream = output.p2_stream();
    stream
        .write(Bytes::from("x\n".repeat(WRITE_LIMIT / 2)))
        .unwrap();
    stream
        .write(Bytes::from_static(b"final failure tail"))
        .unwrap();
    drop(stream);
    drop(output);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !logs
        .records("guest")
        .iter()
        .any(|record| record.message == "final failure tail")
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let records = logs.records("guest");
    assert_eq!(records.len(), crate::logs::RECORDS_PER_PLUGIN);
    assert_eq!(records.last().unwrap().message, "final failure tail");
    assert!(
        records
            .iter()
            .all(|record| record.level == LogLevel::Warning)
    );
}
