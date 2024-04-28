extern crate mpv;
mod anime_dl;
mod anime_find;

use getopts::Options;
use std::path::Path;
use std::process::exit;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;

use pbr::{MultiBar, Pipe, ProgressBar, Units};
//use std::slice::Join;

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
    let mut dccpackages = vec![];
    //let mut packlist = vec![];
    //let mut botlist = vec![];

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

        dccpackages.push(package);
    }

    let mut channel_senders  = vec![];
    let mut multi_bar = MultiBar::new();
    let mut multi_bar_handles = vec![];
    let (status_bar_sender, status_bar_receiver) = channel();
    for i in 0..dccpackages.len() { //create bars for all our downloads
        let (sender, receiver) = channel();
        let handle;
        let mut progress_bar = multi_bar.create_bar(dccpackages[i].sizekbits as u64);

        progress_bar.set_units(Units::Bytes);
        progress_bar.message(&format!("{}: ", dccpackages[i].filename));

        let status_bar_sender_clone = status_bar_sender.clone();
        handle = thread::spawn(move || {
            update_bar(&mut progress_bar, receiver, status_bar_sender_clone);
        });

        channel_senders.push(sender);
        multi_bar_handles.push(handle);
    }

    let mut status_bar = multi_bar.create_bar(dccpackages.len() as u64);
    status_bar.set_units(Units::Default);
    status_bar.message(&format!("{}: ", "Waiting..."));
    let status_bar_handle = thread::spawn(move || {
        update_status_bar(&mut status_bar, status_bar_receiver);
    });
    multi_bar_handles.push(status_bar_handle);

    let _ = thread::spawn(move || {
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
        video_handle = Some(play_video(dccpackages.into_iter().map(|package| package.filename).collect()));
    }

    match anime_dl::connect_and_download(irc_request, channel_senders, status_bar_sender) {
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

fn update_status_bar(progress_bar: &mut ProgressBar<Pipe>, receiver: Receiver<String>) {
    progress_bar.tick();
    let mut progress = match receiver.recv() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Error updating status bar");
            exit(1);
        },
    };

    while !progress.eq("Success") {
        progress_bar.tick();
        if progress.eq("Episode Finished Downloading") {
            progress_bar.inc();
        }

        progress_bar.message(&format!("{} ", progress));
        progress = match receiver.recv() {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Error updating status bar");
                exit(1);
            },
        };
    }
    progress_bar.message(&format!("{} ", progress));
    progress_bar.finish();
}

fn update_bar(progress_bar: &mut ProgressBar<Pipe>, receiver: Receiver<i64>, status_bar_sender: Sender<String>) {
    progress_bar.tick();
    let mut progress = match receiver.recv() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Error updating progress bar");
            exit(1);
        },
    };
    //println!("{} progress, progress);
    while progress > 0 {
        progress_bar.set(progress as u64);
        progress = match receiver.recv() {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Error updating progress bar");
                exit(1);
            },
        };
    }

    status_bar_sender.send("Episode Finished Downloading".to_string()).unwrap();
    progress_bar.finish();
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

fn play_video(filenames: Vec<String>) -> thread::JoinHandle<()>{
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_secs(5));
        let mut i = 0;
        let mut timeout = 0;
        let mut filename = &filenames[i];
        let video_path: &Path = Path::new(&filename);
        while timeout < 5 { //Initial connection waiting
            if !video_path.is_file() {
                timeout += 1;
                thread::sleep(std::time::Duration::from_secs(5));
            } else {
                break;
            }
        }
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
                    //println!("{:?}", event);
                    match event {
                        mpv::Event::Shutdown => {
                            break 'main;
                        }
                        mpv::Event::Idle => {
                            if i >= filenames.len() {
                                break 'main;
                            }
                        }
                        mpv::Event::EndFile(Ok(mpv::EndFileReason::MPV_END_FILE_REASON_EOF)) => {
                            i += 1;
                            if i >= filenames.len() {
                                break 'main;
                            }
                            filename = &filenames[i];
                            let next_video_path = Path::new(&filename);
                            if next_video_path.is_file() {
                                let next_video_path = next_video_path
                                    .to_str()
                                    .expect("Expected a string for Path, got None");
                                mpv.command(&["loadfile", next_video_path as &str])
                                    .expect("Error loading file");
                            } else {
                                eprintln!(
                                    "A file is required; {} is not a valid file",
                                    next_video_path.to_str().unwrap()
                                );
                            }
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
