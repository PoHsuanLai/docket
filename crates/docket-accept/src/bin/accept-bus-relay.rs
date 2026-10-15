//! A connection to the private bus that the bus sees as another process: the harness hands this
//! program one end of a socket pair as its standard input, and it copies bytes between that and
//! the bus socket named on its command line. The bus learns the caller's pid when it accepts a
//! connection, so a connection the harness makes through the relay is the relay's process, and
//! the harness can place that process in a unit of its own in the fake proc root. One test
//! process can then be two callers (the shell and an agent host) the way two processes are in a
//! running desktop. File descriptors sent over the bus are not carried.

use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;

fn copy(mut from: UnixStream, mut to: UnixStream) {
    let mut buffer = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if to.write_all(&buffer[..n]).is_err() {
                    break;
                }
            }
        }
    }
    let _ = to.shutdown(std::net::Shutdown::Write);
}

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("accept-bus-relay: give the bus socket's path");
        std::process::exit(2);
    };
    let Ok(stdin) = std::io::stdin().as_fd().try_clone_to_owned() else {
        eprintln!("accept-bus-relay: standard input is not a descriptor");
        std::process::exit(2);
    };
    let harness = UnixStream::from(stdin);
    let Ok(bus) = UnixStream::connect(&path) else {
        eprintln!("accept-bus-relay: cannot reach the bus at {path}");
        std::process::exit(1);
    };
    let (Ok(harness_in), Ok(bus_in)) = (harness.try_clone(), bus.try_clone()) else {
        std::process::exit(1);
    };
    let up = std::thread::spawn(move || copy(harness_in, bus));
    copy(bus_in, harness);
    let _ = up.join();
}
