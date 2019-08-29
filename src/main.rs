extern crate anime_dl;
extern crate anime_find;

use getopts::Options;
use std::process::exit;

static IRC_SERVER: &str = "irc.rizon.net:6667";
static IRC_CHANNEL: &str = "nibl";
static IRC_NICKNAME: &str = "randomRustacean";

fn print_usage(program: &str, opts: Options) {
    let msg = opts.short_usage(&program);
    print!("{}", opts.usage(&msg));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let program = args[0].clone();
    let mut opts = Options::new();
    opts.reqopt("q", "query", "Query to run", "QUERY")
        .optopt("e", "episode", "Episode number", "NUMBER")
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
    let episode: Option<u16> = match matches.opt_str("e") {
        Some(ep) => Some(parse_episode(ep)),
        None => None,
    };
    let package = match anime_find::find_package(&query, &episode) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
    let irc_request = anime_dl::IRCRequest {
        server: IRC_SERVER.to_string(),
        channel: IRC_CHANNEL.to_string(),
        nickname: IRC_NICKNAME.to_string(),
        bot: package.bot,
        packages: vec![package.number.to_string()],
    };
    match anime_dl::connect_and_download(irc_request) {
        Ok(count) => println!("Downloaded successfuly {} files", count),
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
}

fn parse_episode(episode: String) -> u16 {
    match episode.parse::<u16>() {
        Ok(e) => e,
        Err(_) => {
            eprintln!("Episode number must be numeric.");
            exit(1);
        }
    }
}
