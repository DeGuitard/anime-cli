extern crate mpv;
mod anime_dl;
mod anime_find;

use getopts::Options;
use std::path::Path;
use std::process::exit;
use std::thread;

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
        .optopt("b", "batch", "Batch end number", "NUMBER")
        .optopt("r", "resolution", "Resolution", "NUMBER")
        .optflag("n", "noshow", "No auto viewer")
        .optflag("h", "help", "print this help menu");

    // Unfortunately, cannot use getopts to check for a single optional flag
    // https://github.com/rust-lang-nursery/getopts/issues/46
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_usage(&program, opts);
        exit(0);
    }
    let mut batching = false;
    if args.contains(&"-b".to_string()) || args.contains(&"--batch".to_string()) {
        batching = true;
    }
    let mut noshow = false;
    if args.contains(&"-n".to_string()) || args.contains(&"--noshow".to_string()) {
        noshow = true;
    }

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(error) => {
            eprintln!("{}.", error);
            eprintln!("{}", opts.short_usage(&program));
            exit(1);
        }
    };

    let resolution: Option<u16> = match matches.opt_str("r").as_ref().map(String::as_str) {
        Some("0") => None,
        Some(ep) => Some(parse_number(String::from(ep))),
        None => Some(720),
    };

    let queryres: String = match resolution {
        Some(x) => format!(" {}", x),
        None => "".to_string(),
    };

    let query = matches.opt_str("q").unwrap() + queryres.as_str();
    //println!("{}", query);
    let episode: Option<u16> = match matches.opt_str("e") {
        Some(ep) => Some(parse_number(ep)),
        None => None
    };

    let mut batch =  matches.opt_str("b").map(|ep|parse_number(ep));
    if batch.is_some() && batch.unwrap() < episode.unwrap_or(1) {
        batch = episode;
    }
    let mut packlist = vec![];
    let mut botlist = vec![];

    for i in episode.unwrap_or(1)..batch.unwrap_or(episode.unwrap_or(1)) + 1 {
        if episode.is_some() || batch.is_some() {
            println!("Searching for {} episode {}", query, i);
        } else {
            println!("Searching for {}", query);
        }
        let package = match anime_find::find_package(&query, &episode.or(batch).and(Some(i))) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                exit(1);
            }
        };

        packlist.push(package.number.to_string());
        botlist.push(package.bot);
    }
    let irc_request = anime_dl::IRCRequest {
        server: IRC_SERVER.to_string(),
        channel: IRC_CHANNEL.to_string(),
        nickname: IRC_NICKNAME.to_string(),
        bot: botlist,
        packages: packlist,
    };
    /*
    println!(
        "{:?} {:?} {:?} {:?} {:?}",
        irc_request.server,
        irc_request.channel,
        irc_request.nickname,
        irc_request.bot,
        irc_request.packages
    );*/

    if !batching && !noshow {
        match anime_dl::connect_and_download(irc_request, play_video) {
            Ok(_) => exit(0),
            Err(e) => {
                eprintln!("{}", e);
                exit(1);
            }
        };
    } else {
        // batch
        match anime_dl::connect_and_download(irc_request, do_nothing) {
            Ok(_) => exit(0),
            Err(e) => {
                eprintln!("{}", e);
                exit(1);
            }
        };
    }
}

fn parse_number(episode: String) -> u16 {
    match episode.parse::<u16>() {
        Ok(e) => e,
        Err(_) => {
            eprintln!("Episode number must be numeric.");
            exit(1);
        }
    }
}

fn do_nothing(_filename: String) -> thread::JoinHandle<()>{
    thread::spawn( move || {

    })
}

fn play_video(filename: String) -> thread::JoinHandle<()>{
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_secs(5));
        let video_path: &Path = Path::new(&filename);
        let mut mpv_builder = mpv::MpvHandlerBuilder::new().expect("Failed to init MPV builder");
        if video_path.is_file() {
            let video_path = video_path
                .to_str()
                .expect("Expected a string for Path, got None");
            mpv_builder.set_option("osc", true).unwrap();
            mpv_builder
                .set_option("input-default-bindings", true)
                .unwrap();
            mpv_builder.set_option("input-vo-keyboard", true).unwrap();
            let mut mpv = mpv_builder.build().expect("Failed to build MPV handler");
            mpv.command(&["loadfile", video_path as &str])
                .expect("Error loading file");
            'main: loop {
                while let Some(event) = mpv.wait_event(0.0) {
                    match event {
                        mpv::Event::Shutdown | mpv::Event::Idle => {
                            break 'main;
                        }
                        _ => {}
                    };
                }
            }
        } else {
            eprintln!(
                "A file is required; {} is not a valid file",
                video_path.to_str().unwrap()
            );
        }
    })
}
