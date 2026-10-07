//! The wire over a byte stream, for the binary (stdin and stdout) and for tests (the two ends of
//! an in-memory duplex): newline-delimited JSON, nothing else on the stream.

use crate::server::Ticks;
use crate::wire::{Wire, WireClosed};
use prov::UnixSeconds;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, Lines};

/// A `Wire` over a reader and a writer.
#[derive(Debug)]
pub struct LineWire<R, W> {
    lines: Lines<R>,
    out: W,
}

impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> LineWire<R, W> {
    /// Reads lines from `reader`, writes them to `writer`.
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            lines: reader.lines(),
            out: writer,
        }
    }
}

impl<R: AsyncBufRead + Unpin + Send, W: AsyncWrite + Unpin + Send> Wire for LineWire<R, W> {
    async fn read_line(&mut self) -> Option<String> {
        // `next_line` is cancel-safe: a dropped read loses no input.
        loop {
            match self.lines.next_line().await {
                Ok(Some(line)) if line.trim().is_empty() => {}
                Ok(line) => return line,
                Err(_) => return None,
            }
        }
    }

    async fn write_line(&mut self, line: String) -> Result<(), WireClosed> {
        let mut bytes = line.into_bytes();
        bytes.push(b'\n');
        self.out.write_all(&bytes).await.map_err(|_| WireClosed)?;
        self.out.flush().await.map_err(|_| WireClosed)
    }
}

/// The system clock, for the binary.
#[derive(Debug, Clone, Copy)]
pub struct SystemTicks;

impl Ticks for SystemTicks {
    fn now(&self) -> UnixSeconds {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        UnixSeconds(i64::try_from(secs).unwrap_or(i64::MAX))
    }
}
