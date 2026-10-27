//! The wire over a byte stream, for the binary (stdin and stdout) and for tests (the two ends of
//! an in-memory duplex): newline-delimited JSON, nothing else on the stream.

use crate::wire::{Wire, WireClosed};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// The longest line a peer may send, in bytes: the agent child's stdout and the editor's input
/// both end at it. A line past it is not a message, and nothing buffers it.
pub const LINE_MAX: usize = 4 * 1024 * 1024;

/// Why a line could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LineFault {
    /// The peer sent more than the cap before a newline.
    #[error("a line over {cap} bytes")]
    TooLong {
        /// The cap that was passed.
        cap: usize,
    },
    /// The line is not UTF-8.
    #[error("a line that is not text")]
    NotText,
    /// The stream failed.
    #[error("the stream failed: {0}")]
    Io(#[from] std::io::Error),
}

/// The next line of `reader`, without its newline (or CR), holding at most `cap` bytes.
/// `buf` keeps what has been read of an unfinished line between calls, so a dropped read loses
/// no input and a retry carries on. `None` is the end of the stream. A line over the cap is an
/// error; the stream is then in the middle of it and is to be closed, not read on.
pub async fn read_capped<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    cap: usize,
) -> Result<Option<String>, LineFault> {
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return match buf.is_empty() {
                true => Ok(None),
                false => text_of(std::mem::take(buf)).map(Some),
            };
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let take = newline.unwrap_or(chunk.len());
        if buf.len().saturating_add(take) > cap {
            buf.clear();
            return Err(LineFault::TooLong { cap });
        }
        buf.extend_from_slice(&chunk[..take]);
        std::pin::Pin::new(&mut *reader).consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            return text_of(std::mem::take(buf)).map(Some);
        }
    }
}

fn text_of(mut bytes: Vec<u8>) -> Result<String, LineFault> {
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(|_| LineFault::NotText)
}

/// A `Wire` over a reader and a writer.
#[derive(Debug)]
pub struct LineWire<R, W> {
    reader: R,
    pending: Vec<u8>,
    cap: usize,
    out: W,
}

impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> LineWire<R, W> {
    /// Reads lines from `reader`, writes them to `writer`; a line longer than [`LINE_MAX`] ends
    /// the wire.
    pub fn new(reader: R, writer: W) -> Self {
        Self::capped(reader, writer, LINE_MAX)
    }

    /// The same, with lines held to `cap` bytes.
    pub fn capped(reader: R, writer: W, cap: usize) -> Self {
        Self {
            reader,
            pending: Vec::new(),
            cap,
            out: writer,
        }
    }
}

impl<R: AsyncBufRead + Unpin + Send, W: AsyncWrite + Unpin + Send> Wire for LineWire<R, W> {
    async fn read_line(&mut self) -> Option<String> {
        // `read_capped` keeps a partial line in `pending`: a dropped read loses no input.
        loop {
            match read_capped(&mut self.reader, &mut self.pending, self.cap).await {
                Ok(Some(line)) if line.trim().is_empty() => {}
                Ok(line) => return line,
                Err(fault) => {
                    // The wire is closed on a line it cannot take; the peer is not told why.
                    tracing::warn!(%fault, "closing the wire");
                    return None;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn lines_come_whole_and_a_line_over_the_cap_ends_the_wire() {
        let input: &[u8] = b"one\r\n\ntwo\n0123456789\nrest";
        let mut wire = LineWire::capped(BufReader::new(input), Vec::new(), 8);
        assert_eq!(wire.read_line().await.as_deref(), Some("one"));
        assert_eq!(wire.read_line().await.as_deref(), Some("two"));
        // Ten bytes against a cap of eight: closed, not truncated and not read on.
        assert_eq!(wire.read_line().await, None);
    }

    #[tokio::test]
    async fn an_unfinished_last_line_is_still_a_line() {
        let mut buf = Vec::new();
        let mut reader = BufReader::new(&b"tail"[..]);
        assert_eq!(
            read_capped(&mut reader, &mut buf, 8).await.expect("line"),
            Some("tail".to_owned())
        );
        assert_eq!(
            read_capped(&mut reader, &mut buf, 8).await.expect("end"),
            None
        );
    }
}
