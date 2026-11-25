//! One tracing setup for every binary.
//!
//! Filter with `VICTORY_LOG` (falls back to `RUST_LOG`), e.g.
//! `VICTORY_LOG=victory_loader=trace,info`. Debug and trace output exist
//! only in builds with the `diagnostics` feature; a plain release build
//! logs info and above whatever the filter asks for.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use std::io::{BufWriter, Write};
use std::sync::{Arc, OnceLock, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc};
use std::time::{Duration, Instant};

const QUEUE: usize = 1024;
const MAX_EVENT: usize = 16 * 1024;
static ACTIVE: OnceLock<AsyncWriter> = OnceLock::new();

enum Message { Data(Vec<u8>), Flush(mpsc::SyncSender<()>) }

#[derive(Clone)]
struct AsyncWriter {
    sender: mpsc::SyncSender<Message>,
    lost: Arc<AtomicU64>,
    truncated: Arc<AtomicU64>,
}

struct EventWriter { writer: AsyncWriter, bytes: Vec<u8>, truncated: bool }

impl Write for EventWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let n = bytes.len().min(MAX_EVENT.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..n]);
        self.truncated |= n != bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

impl Drop for EventWriter {
    fn drop(&mut self) {
        if self.truncated {
            self.writer.truncated.fetch_add(1, Ordering::Relaxed);
            if let Some(last) = self.bytes.last_mut() { *last = b'\n'; }
        }
        if !self.bytes.is_empty() && self.writer.sender.try_send(Message::Data(std::mem::take(&mut self.bytes))).is_err() {
            self.writer.lost.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for AsyncWriter {
    type Writer = EventWriter;
    fn make_writer(&'a self) -> EventWriter {
        EventWriter { writer: self.clone(), bytes: Vec::with_capacity(256), truncated: false }
    }
}

/// Flush on ordinary return; explicit flush also covers title process exit.
pub struct LogGuard {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        flush();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

/// Used at orderly title suspension. Nothing on a render thread waits for
/// disk I/O; at shutdown this waits at most one second for the log worker.
pub fn flush() {
    let Some(writer) = ACTIVE.get() else { return; };
    let (tx, rx) = mpsc::sync_channel(1);
    let began = Instant::now();
    let mut message = Message::Flush(tx);
    loop {
        match writer.sender.try_send(message) {
            Ok(()) => break,
            Err(mpsc::TrySendError::Full(returned)) if began.elapsed() < Duration::from_millis(250) => {
                message = returned;
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(_) => return,
        }
    }
    let _ = rx.recv_timeout(Duration::from_secs(1).saturating_sub(began.elapsed()));
}

fn asynchronous() -> (AsyncWriter, LogGuard) {
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let writer = AsyncWriter { sender: tx, lost: Arc::new(AtomicU64::new(0)), truncated: Arc::new(AtomicU64::new(0)) };
    let (lost, truncated) = (writer.lost.clone(), writer.truncated.clone());
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let worker = std::thread::Builder::new().name("starport-log".into()).spawn(move || {
        let mut out = BufWriter::with_capacity(64 * 1024, std::io::stderr());
        let mut flushed = Instant::now();
        let mut reported = (0, 0);
        loop {
            let message = rx.recv_timeout(Duration::from_millis(250));
            match message {
                Ok(Message::Data(bytes)) => { let _ = out.write_all(&bytes); }
                Ok(Message::Flush(ack)) => {
                    let counts = (lost.load(Ordering::Relaxed), truncated.load(Ordering::Relaxed));
                    if counts != reported {
                        let _ = writeln!(out, "STARPORT_LOG_LOSS dropped={} truncated={} (cumulative)", counts.0, counts.1);
                        reported = counts;
                    }
                    let _ = out.flush(); let _ = ack.send(());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if flushed.elapsed() >= Duration::from_millis(250) || stopping.load(Ordering::Acquire) {
                let counts = (lost.load(Ordering::Relaxed), truncated.load(Ordering::Relaxed));
                if counts != reported {
                    let _ = writeln!(out, "STARPORT_LOG_LOSS dropped={} truncated={} (cumulative; bounded nonblocking recorder)", counts.0, counts.1);
                    reported = counts;
                }
                let _ = out.flush();
                flushed = Instant::now();
            }
            if stopping.load(Ordering::Acquire) { break; }
        }
        // Bounded final drain; other title threads may still be logging.
        for _ in 0..QUEUE {
            match rx.try_recv() {
                Ok(Message::Data(bytes)) => { let _ = out.write_all(&bytes); }
                Ok(Message::Flush(ack)) => { let _ = out.flush(); let _ = ack.send(()); }
                Err(_) => break,
            }
        }
        let _ = out.flush();
    }).expect("log worker");
    (writer, LogGuard { stop, worker: Some(worker) })
}

/// The most verbose level this build can log.
const CAP: LevelFilter = if cfg!(feature = "diagnostics") {
    LevelFilter::TRACE
} else {
    LevelFilter::INFO
};

pub fn init(default_level: &str) -> LogGuard {
    let filter = std::env::var("VICTORY_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|v| EnvFilter::try_new(v).ok())
        .unwrap_or_else(|| EnvFilter::new(default_level));
    let (writer, guard) = asynchronous();
    let _ = ACTIVE.set(writer.clone());
    let _ = tracing_subscriber::registry()
        .with(CAP)
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_ansi(false)
                .with_writer(writer),
        )
        .try_init();
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::fmt::MakeWriter;

    #[test]
    fn recorder_never_blocks_a_full_queue_and_reports_loss() {
        let (tx, rx) = mpsc::sync_channel(1);
        let writer = AsyncWriter { sender: tx, lost: Arc::new(AtomicU64::new(0)), truncated: Arc::new(AtomicU64::new(0)) };
        { let mut event = writer.make_writer(); event.write_all(b"first").unwrap(); }
        { let mut event = writer.make_writer(); event.write_all(b"second").unwrap(); }
        assert_eq!(writer.lost.load(Ordering::Relaxed), 1);
        assert!(matches!(rx.try_recv(), Ok(Message::Data(bytes)) if bytes == b"first"));
        { let mut event = writer.make_writer(); event.write_all(&vec![b'x'; MAX_EVENT+1]).unwrap(); }
        assert!(matches!(rx.try_recv(), Ok(Message::Data(bytes)) if bytes.len() == MAX_EVENT));
        assert_eq!(writer.truncated.load(Ordering::Relaxed), 1);
    }
}
