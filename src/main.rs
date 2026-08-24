fn main() {
    if let Err(err) = native_lsp::server::run() {
        eprintln!("native-lsp exited: {err}");
        std::process::exit(1);
    }
}
