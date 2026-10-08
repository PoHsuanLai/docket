//! `docket-net-forward run --listen 127.0.0.1:<port> --socket <path> -- <program> [args...]`
//!
//! The forwarder an endpoint-only agent starts behind, which bulkhead provides as a function;
//! the host starts it from beside itself (`agent::sibling`).

fn main() -> std::process::ExitCode {
    bulkhead::cli::forward_main("docket-net-forward")
}
