//! One HTTP/1.1 POST over a plain `TcpStream`: connect with a timeout, write the request, read
//! the status line, close. Nothing else of the reply is read.

use super::config::Endpoint;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// More than any status line; a reply that has none in this much is not HTTP.
const STATUS_LINE_LIMIT: u64 = 512;

/// Why a POST failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PostError {
    #[error("the collector could not be reached: {0}")]
    Connect(io::Error),
    #[error("the collector's host has no address")]
    NoAddress,
    #[error("the connection failed: {0}")]
    Io(io::Error),
    #[error("the collector did not answer with an HTTP status line")]
    NotHttp,
    #[error("the collector answered {0}")]
    Status(u16),
}

/// POSTs `body` as `application/json` to `endpoint`; any 2xx answer is success.
pub(crate) fn post(endpoint: &Endpoint, body: &[u8]) -> Result<(), PostError> {
    let addr = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(PostError::Connect)?
        .next()
        .ok_or(PostError::NoAddress)?;
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(PostError::Connect)?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .and_then(|()| stream.set_read_timeout(Some(IO_TIMEOUT)))
        .map_err(PostError::Io)?;
    let head = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.path,
        endpoint.host_header(),
        body.len()
    );
    send(&stream, &head, body).map_err(PostError::Io)?;
    let mut line = String::new();
    BufReader::new((&stream).take(STATUS_LINE_LIMIT))
        .read_line(&mut line)
        .map_err(PostError::Io)?;
    status_of(&line)
}

/// Writes the request head and body.
fn send(stream: &TcpStream, head: &str, body: &[u8]) -> io::Result<()> {
    let mut writer = stream;
    writer.write_all(head.as_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

/// Success for a 2xx status line, the status or `NotHttp` otherwise.
fn status_of(line: &str) -> Result<(), PostError> {
    let mut words = line.split_whitespace();
    let version = words.next().unwrap_or("");
    let code = words.next().and_then(|code| code.parse::<u16>().ok());
    match code {
        Some(code) if version.starts_with("HTTP/") && (200..300).contains(&code) => Ok(()),
        Some(code) if version.starts_with("HTTP/") => Err(PostError::Status(code)),
        _ => Err(PostError::NotHttp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_2xx_status_line_is_success() {
        let ok = |line| status_of(line).is_ok();
        assert!(ok("HTTP/1.1 200 OK\r\n"));
        assert!(ok("HTTP/1.1 204 No Content\r\n"));
        assert!(matches!(
            status_of("HTTP/1.1 503 Service Unavailable\r\n"),
            Err(PostError::Status(503))
        ));
        for line in ["", "\r\n", "garbage", "HTTP/1.1\r\n", "SSH-2.0 200 x"] {
            assert!(
                matches!(status_of(line), Err(PostError::NotHttp)),
                "{line:?}"
            );
        }
    }
}
