//! Controlled process fixture: runs the production TLS node entry point.
fn main() -> std::io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let config = args
        .next()
        .filter(|_| args.next().is_none())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "one fixture config path required",
            )
        })?;
    anima_daemon::node_serve(std::path::Path::new(&config))
}
