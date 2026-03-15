mod app;
mod battery;
mod config;
mod gpu;
mod system;

fn main() {
    if let Err(e) = app::run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
