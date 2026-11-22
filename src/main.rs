mod annotate;
mod copy;
mod devices;
mod dto;
mod encode;
mod engine;
mod error;
mod geom;
mod library;
mod media;
mod naming;
mod paths;
mod platform;
mod report;
mod screen;
mod session;
mod settings;
mod share;
mod ui;
mod veil;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("serve") {
        std::process::exit(share::serve_cli(&args[1..]));
    }
    ui::run();
}
