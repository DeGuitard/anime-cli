mod anime_dl;
mod anime_find;

use getopts::Options;
use std::process::exit;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

static IRC_SERVER: &str = "irc.rizon.net:6667";
static IRC_CHANNEL: &str = "nibl";
static IRC_NICKNAME: &str = "randomRustacean";

fn print_usage(program: &str, opts: Options) {
    let msg = opts.short_usage(program);
    print!("{}", opts.usage(&msg));
}

fn main() {
    // Set up graceful shutdown handler
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    ctrlc::set_handler(move || {
        shutdown_clone.store(true, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    let args: Vec<String> = std::env::args().collect();
    let program = args[0].clone();
    let mut opts = Options::new();
    opts.reqopt("q", "query", "Query to run", "QUERY")
        .optopt(
            "e",
            "episodes",
            "Episode number(s), separated with comma",
            "NUMBER",
        )
        .optflag("h", "help", "print this help menu");

    // Unfortunately, cannot use getopts to check for a single optional flag
    // https://github.com/rust-lang-nursery/getopts/issues/46
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_usage(&program, opts);
        exit(0);
    }

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(error) => {
            eprintln!("{}.", error);
            eprintln!("{}", opts.short_usage(&program));
            exit(1);
        }
    };

    let query = matches.opt_str("q").unwrap();
    let packages = match matches.opt_str("e") {
        Some(ep) => match anime_find::find_packages(&query, &parse_episodes(ep)) {
            Ok(pkgs) => pkgs,
            Err(e) => {
                eprintln!("Error: {}", e);
                exit(1);
            }
        },
        None => match anime_find::find_package(&query, &None) {
            Ok(pkg) => vec![pkg],
            Err(e) => {
                eprintln!("Error: {}", e);
                exit(1);
            }
        },
    };

    let mut packages_by_bot = std::collections::HashMap::new();
    for package in packages.iter() {
        packages_by_bot
            .entry(&package.bot)
            .or_insert(vec![])
            .push(package.number);
    }

    for (bot, packages) in packages_by_bot {
        // Check if shutdown was requested before starting new bot connection
        if shutdown.load(Ordering::SeqCst) {
            eprintln!("\nShutdown requested, exiting gracefully...");
            exit(130); // Standard exit code for SIGINT
        }

        let irc_request = anime_dl::IRCRequest {
            server: IRC_SERVER.to_string(),
            channel: IRC_CHANNEL.to_string(),
            nickname: IRC_NICKNAME.to_string(),
            bot: bot.to_owned(),
            packages,
        };
        match anime_dl::connect_and_download(irc_request, shutdown.clone(), |_| ()) {
            Ok(_) => {},
            Err(e) => {
                // Use appropriate exit code for interruption
                let exit_code = if e == "Interrupted by user" { 130 } else { 1 };
                eprintln!("{}", e);
                exit(exit_code);
            }
        };
    }
    exit(0);
}

fn parse_episodes(episodes: String) -> Vec<u16> {
    episodes.split(",").map(parse_episode).collect::<Vec<_>>()
}

fn parse_episode(episode: &str) -> u16 {
    match episode.parse::<u16>() {
        Ok(e) => e,
        Err(_) => {
            eprintln!("Error: '{}' is not a valid episode number. Episode numbers must be positive integers.", episode);
            exit(1);
        }
    }
}
