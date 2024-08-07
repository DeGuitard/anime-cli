#[cfg(feature = "mpv")]
extern crate mpv;
extern crate crossterm;

mod anime_dl;
mod anime_find;
mod anime_watch;

use getopts::Options;
use std::fs;
use std::path::{Path};
use std::ffi::OsStr;
use std::process::exit;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::io;
use std::error::Error;

use pbr::{MultiBar, Pipe, ProgressBar, Units};
use crossterm::terminal::size;
use crossterm::ErrorKind;

static IRC_SERVER: &str = "irc.rizon.net:6667";
static IRC_CHANNEL: &str = "nibl";
static IRC_NICKNAME: &str = "randomRustacean";


const AUDIO_EXTENSIONS: &'static [&'static str] = &["aif", "cda", "mid", "midi", "mp3",
                                                    "mpa", "ogg", "wav", "wma", "wpl"];

const VIDEO_EXTENSIONS: &'static [&'static str] = &["3g2", "3gp", "avi", "flv", "h264",
                                                    "m4v", "mkv", "mov", "mp4", "mpg",
                                                    "mpeg", "rm", "swf", "vob", "wmv"];

pub fn is_valid_media_file(ext: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&ext) || VIDEO_EXTENSIONS.contains(&ext)
}

const ACCEPTABLE_WIDTH_PERCENTAGE: u16 = 50; // Filename only takes up half the screen
const CHAR_THRESH_1: u16 = 55; // Style 1 truncation until this # chars width
const CHAR_THRESH_2: u16 = 35; // Style 2 truncation until this # chars, then turn off bars altogether

fn print_usage(program: &str, opts: Options) {
    let msg = opts.short_usage(&program);
    print!("{}", opts.usage(&msg));
    println!("\n\
    ===================================\n\
    Helpful Tips:                      \n\
    Try to keep your anime name simple \n\
    and use quotes when you use -q     \n\
    e.g. \"sakamoto\"                  \n\
                                       \n\
    Common resolutions 480/720/1080    \n\
                                       \n\
    Batch end number means last episode\n\
    in a range of episodes             \n\
      e.g. episode ------------> batch \n\
      everything from 1 -------> 10    \n\
                                       \n\
    You can apply default resolution   \n\
    and default batch # with a blank   \n\
    ===================================\n
    ");
}

pub fn get_cli_input(prompt: &str) -> String {
    println!("{}", prompt);
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => {},
        Err(e) => {
            eprintln!("{}", e);
            eprintln!("Please enter a normal input");
            exit(1);
        }
    }
    input.to_string().replace(|c: char| c == '\n' || c == '\r', "")
}

fn main() {
    let args: Vec<String> = std::env::args().collect(); // We collect args here
    let program = args[0].clone();
    let mut opts = Options::new();
    opts.optopt("q", "query", "Query to run", "QUERY")
        .optopt("e", "episode", "Episode number", "NUMBER")
        .optopt("b", "batch", "Batch end number", "NUMBER")
        .optopt("r", "resolution", "Resolution", "NUMBER")
        .optflag("n", "noshow", "No auto viewer")
        .optflag("x", "explore", "Browse local collection")
        .optflag("h", "help", "print this help menu");

    // Unfortunately, cannot use getopts to check for a single optional flag
    // https://github.com/rust-lang-nursery/getopts/issues/46
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_usage(&program, opts);
        exit(0);
    }
    if args.contains(&"-x".to_string()) || args.contains(&"--explore".to_string()) {
        match anime_watch::browse_anime_listings() {
            Ok(_) => {},
            Err(_) => { eprintln!("Could not spawn virtual screen"); }
        };
        exit(0);
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

    let cli = if args.len() > 1 { true } else { false }; // Are we in cli mode or prompt mode?

    let mut query: String;
    let resolution: Option<u16>;
    let episode: Option<u16>;
    let mut batch: Option<u16>;

    if cli { // Get user input
        resolution = match matches.opt_str("r").as_ref().map(String::as_str) {
            Some("0") => None,
            Some(r) => Some(parse_number(String::from(r))),
            None => Some(720),
        };

        query = matches.opt_str("q").unwrap();

        episode = match matches.opt_str("e") {
            Some(ep) => Some(parse_number(ep)),
            None => None
        };

        batch = match matches.opt_str("b") {
            Some(b) => Some(parse_number(b)),
            None => None
        }

    } else {
        println!("Welcome to anime-cli");
        let start = get_cli_input("Enter 'x' to browse, anything else to search");
        match start.as_str() {
            "x" | "X" => {
                match anime_watch::browse_anime_listings() {
                    Ok(_) => {},
                    Err(_) => { eprintln!("Could not spawn virtual screen"); }
                };
                exit(0);
            },
            _ => { }
        }
        println!("Default resolution: None | Episode: None | Batch = episode");
        println!("Resolution shortcut: 1 => 480p | 2 => 720p | 3 => 1080p");
        query = get_cli_input("Anime/Movie name: ");
        resolution =  match parse_number(get_cli_input("Resolution: ")) {
            0 => None,
            1 => Some(480),
            2 => Some(720),
            3 => Some(1080),
            r => Some(r),
        };
        episode = match parse_number(get_cli_input("Episode number: ")) {
            0 => None,
            e => Some(e),
        };
        batch = match parse_number(get_cli_input("Batch Ep End Number: ")) {
            0 => { if episode.is_some() { episode } else { None } },
            b => Some(b),
        };
    }

    query = query + match resolution { // If resolution entered, add a resolution to the query
        Some(x) => format!(" {}", x),
        None => "".to_string(),
    }.as_str();

    if batch.is_some() && batch.unwrap() < episode.unwrap_or(1) { // Make sure batch end is never smaller than episode start
        batch = episode;
    }

    let mut dccpackages = vec![];

    let mut num_episodes = 0;  // Search for packs, verify it is media, and add to a list
    for i in episode.unwrap_or(1)..batch.unwrap_or(episode.unwrap_or(1)) + 1 {
        if episode.is_some() || batch.is_some() {
            println!("Searching for {} episode {}", query, i);
        } else {
            println!("Searching for {}", query);
        }
        match anime_find::find_package(&query, &episode.or(batch).and(Some(i))) {
            Ok(p) => {
                match Path::new(&p.filename).extension().and_then(OsStr::to_str) {
                    Some(ext) => {
                        if !is_valid_media_file(ext) {
                            eprintln!("Warning, this is not a media file! Skipping");
                        } else {
                            dccpackages.push(p);
                            num_episodes += 1;
                        }
                    },
                    _ => { eprintln!("Warning, this file has no extension, skipping"); }
                }
            },
            Err(e) => {
                eprintln!("{}", e);
            }
        };
    }

    if num_episodes == 0 { exit(1); }

    match fs::create_dir(&query) { // organize
        Ok(_) => println!{"Created folder {}", &query},
        Err(_) => eprintln!{"Could not create a new folder, does it exist?"},
    };
    let dir_path= Path::new(&query).to_owned();

    let terminal_dimensions = size();

    let mut channel_senders  = vec![];
    let mut multi_bar = MultiBar::new();
    let mut multi_bar_handles = vec![];
    let (status_bar_sender, status_bar_receiver) = channel();

    let mut safe_to_spawn_bar = true; // Even if one bar is safe to spawn, sending stdout outputs will interfere with the bars
    for i in 0..dccpackages.len() { //create bars for all our downloads
        let (sender, receiver) = channel();
        let handle;

        let pb_message;
        match terminal_dimensions {
            Ok((w, _)) => {
                let acceptable_length = (w as f64 * (ACCEPTABLE_WIDTH_PERCENTAGE as f64 / 100.0)) as u16;
                if &(dccpackages[i].filename.len() as u16) > &acceptable_length { // trim the filename
                    let first_half = &dccpackages[i].filename[..dccpackages[i].filename.char_indices().nth(acceptable_length as usize / 2).unwrap().0];
                    let second_half = &dccpackages[i].filename[dccpackages[i].filename.char_indices().nth_back(acceptable_length as usize / 2).unwrap().0..];
                    if acceptable_length > CHAR_THRESH_1 {
                        pb_message = format!("{}...{}: ", first_half, second_half);
                    } else if acceptable_length > CHAR_THRESH_2 {
                        pb_message = format!("...{}: ", second_half);
                    } else {
                        pb_message = format!("{} added to list", dccpackages[i].filename);
                        safe_to_spawn_bar = false;
                    }
                } else {
                    pb_message = format!("{}: ", dccpackages[i].filename);
                }
            },
            Err(_) => {
                pb_message = format!("{} added to list", dccpackages[i].filename);
                safe_to_spawn_bar = false;
            },
        };
        let progress_bar;
        if safe_to_spawn_bar {
            let mut pb = multi_bar.create_bar(dccpackages[i].sizekbits as u64);
            pb.set_units(Units::Bytes);
            pb.message(&pb_message);
            progress_bar = Some(pb);
        } else { // If we can't spawn a bar, we just issue normal stdout updates
            progress_bar = None;
            println!("{}", pb_message);
        }

        let status_bar_sender_clone = status_bar_sender.clone();
        handle = thread::spawn(move || { // create an individual thread for each bar in the multibar with its own i/o
            update_bar(progress_bar, receiver, status_bar_sender_clone);
        });

        channel_senders.push(sender);
        multi_bar_handles.push(handle);
    }

    let mut status_bar = None;
    if safe_to_spawn_bar {
        let mut sb = multi_bar.create_bar(dccpackages.len() as u64);
        sb.set_units(Units::Default);
        sb.message(&format!("{}: ", "Waiting..."));
        status_bar = Some(sb);
    }

    let status_bar_handle = thread::spawn(move || {
        update_status_bar(status_bar, status_bar_receiver, terminal_dimensions);
    });
    multi_bar_handles.push(status_bar_handle);

    let _ = thread::spawn(move || { // multi bar listen is blocking
        multi_bar.listen();
    });

    let irc_request = anime_dl::IRCRequest {
        server: IRC_SERVER.to_string(),
        channel: IRC_CHANNEL.to_string(),
        nickname: IRC_NICKNAME.to_string(),
        bot: dccpackages.clone().into_iter().map(|package| package.bot).collect(),
        packages: dccpackages.clone().into_iter().map(|package| package.number.to_string()).collect(),
    };

    let mut video_handle = None;
    if !noshow {
        video_handle =
            if cfg!(feature = "mpv") {
                Some(anime_watch::play_video(dccpackages.into_iter().map(|package| package.filename).collect(), dir_path.clone()))
            } else {
                if num_episodes == 1 { //If we don't have mpv, we'll open the file using default media app. We can't really hook into it so we limit to 1 file so no spam
                    Some(anime_watch::play_video(dccpackages.into_iter().map(|package| package.filename).collect(), dir_path.clone()))
                } else {
                    None
                }
            }
    }

    match anime_dl::connect_and_download(irc_request, channel_senders, status_bar_sender, dir_path.clone()) {
        Ok(_) => {},
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
    if let Some(vh) = video_handle {
        vh.join().unwrap();
    }
    multi_bar_handles.into_iter().for_each(|handle| handle.join().unwrap());
}

fn update_status_bar(progress_bar: Option<ProgressBar<Pipe>>, receiver: Receiver<String>, terminal_dimensions: Result<(u16, u16), ErrorKind>) {
    let trim_message = |length: u16, msg: String| {
        let mut result = msg;
        if length > 0 && result.len() > length as usize {
            let first_half = &result[..result.char_indices().nth(length as usize / 2).unwrap().0];
            let second_half = &result[result.char_indices().nth_back(length as usize / 2).unwrap().0..];
            if length > CHAR_THRESH_1 {
                result = format!("{}...{}: ", first_half, second_half);
            } else if length > CHAR_THRESH_2 {
                result = format!("...{}: ", second_half);
            } else {
                result = format!("Progress...");
            }
        }

        result
    };
    if progress_bar.is_some() {
        let mut acceptable_length = 0;
        match terminal_dimensions {
            Ok((w, _)) => {
                acceptable_length = (w as f64 * (ACCEPTABLE_WIDTH_PERCENTAGE as f64 / 100.0)) as u16;
            }
            Err(_) => { }
        }
        let mut pb = progress_bar.unwrap();
        pb.tick();
        let mut progress = trim_message(acceptable_length, receiver.recv().expect("Error updating status bar"));

        while !progress.eq("Success") {
            pb.tick();
            if progress.eq("Episode Finished Downloading") {
                pb.inc();
            }

            pb.message(&format!("{} ", progress));
            progress = trim_message(acceptable_length, receiver.recv().expect("Error updating status bar"));
        }
        pb.message(&format!("{} ", progress));
        pb.tick();
        pb.finish();
    } else {
        let mut progress = receiver.recv().expect("Error updating status");

        while !progress.eq("Success") {
            println!("{} ", progress);
            progress = receiver.recv().expect("Error updating status");
        }

        println!("{} ", progress);
    }
}

fn update_bar(progress_bar: Option<ProgressBar<Pipe>>, receiver: Receiver<i64>, status_bar_sender: Sender<String>) {
    if progress_bar.is_some() {
        let mut pb = progress_bar.unwrap();
        pb.tick();

        let mut progress = receiver.recv().expect("Error updating progress bar");

        while progress > 0 {
            pb.set(progress as u64);

            progress = receiver.recv().expect("Error updating progress bar");
        }
        pb.finish();
    } else {
        let mut progress = receiver.recv().expect("Error updating progress");

        while progress > 0 {
            progress = receiver.recv().expect("Error updating progress");
        }
    }

    status_bar_sender.send("Episode Finished Downloading".to_string()).unwrap();
}

fn parse_number(str_num: String) -> u16 {
    let c_str_num = str_num.replace(|c: char| !c.is_numeric(), "");
    match c_str_num.parse::<u16>() {
        Ok(e) => e,
        Err(err) => {
            if err.description().eq_ignore_ascii_case("cannot parse integer from empty string") { return 0 }
            eprintln!("Input must be numeric.");
            exit(1);
        }
    }
}