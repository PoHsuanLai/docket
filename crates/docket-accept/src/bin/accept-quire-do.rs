//! `quire-do` as its own process: the packaged program, under a name of the acceptance harness
//! (a test of this package can only name this package's binaries).

fn main() -> std::process::ExitCode {
    docket_cli::program::main()
}
