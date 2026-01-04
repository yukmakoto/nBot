use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogLine {
    pub id: u64,
    pub line: String,
}

#[derive(Default)]
struct LogStoreInner {
    next_id: u64,
    lines: VecDeque<LogLine>,
}

#[derive(Default)]
pub struct LogStore {
    inner: Mutex<LogStoreInner>,
    max_lines: usize,
}

impl LogStore {
    pub fn new(max_lines: usize) -> Self {
        Self {
            inner: Mutex::new(LogStoreInner::default()),
            max_lines: max_lines.max(100).min(200_000),
        }
    }

    pub fn push_line(&self, line: String) {
        let mut inner = self.inner.lock().unwrap();
        inner.next_id = inner.next_id.saturating_add(1);
        let id = inner.next_id;
        inner.lines.push_back(LogLine { id, line });

        while inner.lines.len() > self.max_lines {
            inner.lines.pop_front();
        }
    }

    pub fn snapshot(&self, cursor: Option<u64>, limit: usize) -> (u64, bool, Vec<LogLine>) {
        let limit = limit.max(1).min(5000);
        let inner = self.inner.lock().unwrap();
        let latest = inner.next_id;
        let Some(first) = inner.lines.front() else {
            return (latest, false, Vec::new());
        };
        let earliest = first.id;

        match cursor {
            None => {
                let start = inner.lines.len().saturating_sub(limit);
                let lines = inner.lines.iter().skip(start).cloned().collect::<Vec<_>>();
                (latest, false, lines)
            }
            Some(cur) => {
                let truncated = cur < earliest.saturating_sub(1);
                let mut out: Vec<LogLine> = Vec::new();
                for line in inner.lines.iter() {
                    if line.id > cur {
                        out.push(line.clone());
                        if out.len() >= limit {
                            break;
                        }
                    }
                }
                (latest, truncated, out)
            }
        }
    }
}

#[derive(Clone)]
pub struct TeeMakeWriter {
    store: Arc<LogStore>,
}

impl TeeMakeWriter {
    pub fn new(store: Arc<LogStore>) -> Self {
        Self { store }
    }
}

pub struct TeeWriter {
    store: Arc<LogStore>,
    inner: io::Stdout,
    buf: Vec<u8>,
}

impl TeeWriter {
    fn new(store: Arc<LogStore>) -> Self {
        Self {
            store,
            inner: io::stdout(),
            buf: Vec::new(),
        }
    }

    fn flush_lines(&mut self) {
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let mut line = self.buf.drain(..=pos).collect::<Vec<u8>>();
            if let Some(b'\n') = line.last() {
                line.pop();
            }
            if let Some(b'\r') = line.last() {
                line.pop();
            }
            let s = String::from_utf8_lossy(&line).to_string();
            self.store.push_line(s);
        }
    }
}

impl<'a> MakeWriter<'a> for TeeMakeWriter {
    type Writer = TeeWriter;

    fn make_writer(&'a self) -> Self::Writer {
        TeeWriter::new(self.store.clone())
    }
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.buf.extend_from_slice(&buf[..written]);
        self.flush_lines();
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()?;
        Ok(())
    }
}

