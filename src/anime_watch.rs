#[cfg(feature = "mpv")]
extern crate mpv;
extern crate crossterm;

use std::path::PathBuf;
use std::{fs, thread, env};
use std::process::exit;
use std::ffi::OsStr;
use std::io::{stdout, Write};

use crossterm::{execute, Result, terminal};
use crossterm::event::{poll, read, Event, KeyCode};
use crossterm::cursor::{MoveTo, RestorePosition, SavePosition};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::style::Print;

use crate::{is_valid_media_file};
use std::time::Duration;

const LISTINGS_PER_PAGE: usize = 7;

#[derive(Clone)]
struct AnimeListing {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub episode_count: u32,
    pub is_media: bool,
}

pub fn browse_anime_listings() -> Result<()> {
    execute!(stdout(), EnterAlternateScreen)?;
    let anime_dir = match env::current_dir() {
        Ok(path) => path,
        Err(_) => { eprintln!("Do you have permission to modify this folder?"); exit(1) }
    };

    let mut sub_dir = anime_dir.clone();
    let mut prefix = anime_dir.clone();
    prefix.pop();

    let mut current_page = 1;
    let mut current_selected_row = 0;
    let mut show_empty_folders = false; // allow users to see and delete empty folders and non media items

    'main: loop {
        let listings = get_anime_listings(sub_dir.clone(), show_empty_folders);
        let mut max_pages = listings.len() / LISTINGS_PER_PAGE;
        let mut listings_on_last_page= LISTINGS_PER_PAGE;
        if listings.len() % LISTINGS_PER_PAGE != 0 {
            max_pages += 1;
            listings_on_last_page = listings.len() % LISTINGS_PER_PAGE;
        } else if listings.len() == 0 {
            max_pages = 1;
            listings_on_last_page = 1;
        }

        let tip_cursor_offset = 5; //Lines of text at top
        execute!(stdout(), MoveTo(1, tip_cursor_offset), SavePosition)?;

        let mut clear_input_buffer = false;
        let mut delete_prompt = false;

        'pages: loop {
            execute!(stdout(), terminal::Clear(terminal::ClearType::All))?;
            execute!(stdout(), MoveTo(0,0))?;

            execute!(stdout(), Print(format!("Use arrow keys to navigate\n")))?;
            execute!(stdout(), Print(format!("Enter to Select | Esc to Quit | Del to Delete | S to Show Hidden Items: {}\n", show_empty_folders)))?;
            execute!(stdout(), Print(format!("If you have mpv, hit Q to play all media in current folder\n")))?;
            execute!(stdout(), Print(format!("\\{}\n", sub_dir.strip_prefix(&prefix).unwrap().to_str().unwrap())))?;
            execute!(stdout(), Print(format!("List of animes | Page {} of {}: \n", current_page, max_pages)))?;

            let current_position = (current_page - 1) * LISTINGS_PER_PAGE;
            let clp = |cp| {
                if cp != max_pages {
                    LISTINGS_PER_PAGE
                } else {
                    listings_on_last_page
                }
            };
            if listings.len() > 0 {
                for i in current_position..(current_position + clp(current_page)) {
                    if listings[i].is_dir {
                        execute!(stdout(), Print(format!("[ ] {} | {} episode(s)\n", listings[i].name, listings[i].episode_count)))?;
                    } else {
                        execute!(stdout(), Print(format!("[ ] {}\n", listings[i].name)))?;
                    }
                }
            } else {
                execute!(stdout(), Print(format!("< > Nothing here")))?;
                execute!(stdout(), MoveTo(1, tip_cursor_offset), SavePosition)?;
                current_selected_row = 0;
            }
            execute!(stdout(), RestorePosition)?;

            if clear_input_buffer { // Clear any inputs made during frozen terminal while movie is playing
                'clear: loop {
                    if poll(Duration::from_millis(50))? {
                        match read()? {
                            _ => {}
                        }
                    } else {
                        break 'clear
                    }
                }

                clear_input_buffer = false;
            }

            'input: loop {
                let last_page = current_page;
                let index = current_position + current_selected_row;

                match read()? {
                    Event::Key(event) => {
                        if listings.len() == 0 { // Really make sure that a user can't mess around in empty dir
                            match event.code {
                                KeyCode::Esc => {
                                    if sub_dir == anime_dir {
                                        break 'main; // Bye
                                    } else {
                                        sub_dir.pop();
                                        current_page = 1;
                                        current_selected_row = 0;
                                        break 'pages;
                                    }
                                },
                                _ => { break 'input; }
                            }
                        }
                        if delete_prompt {
                            match event.code {
                                KeyCode::Delete => {
                                    match delete_item(listings[index].clone()) {
                                        Ok(_) => execute!(stdout(), Print(format!("{} successfully deleted\n", listings[index].name)))?,
                                        Err(e) => execute!(stdout(), Print(format!("{}\n", e)))?,
                                    }
                                    match read()? { _ => {} } // wait for acknowledgement

                                    current_page = 1;
                                    current_selected_row = 0;
                                    break 'pages;
                                },
                                _ => {
                                    delete_prompt = false;
                                    break 'input;
                                }
                            }
                        }
                        match event.code {
                            KeyCode::Up => {
                                if current_selected_row != 0 { current_selected_row -= 1; }
                            },
                            KeyCode::Down => {
                                if current_selected_row != clp(current_page) - 1 { current_selected_row += 1; }
                            },
                            KeyCode::Left => {
                                if current_page != 1 { current_page -= 1; }
                            },
                            KeyCode::Right => {
                                if current_page != max_pages { current_page += 1; }
                            },
                            KeyCode::Char('q') | KeyCode::Char('Q') => {
                                if let Some(_) = listings.iter().find(|x| x.is_media) {
                                    let handle = call_play_videos(listings.clone());
                                    execute!(stdout(), terminal::Clear(terminal::ClearType::All))?;
                                    execute!(stdout(), SavePosition, MoveTo(0,0))?;

                                    execute!(stdout(), Print(format!("Now playing all episodes in {}\n", sub_dir.file_name().and_then(OsStr::to_str).unwrap().to_owned())))?;
                                    clear_input_buffer = true;
                                    handle.join().unwrap();
                                }
                                break 'input;
                            },
                            KeyCode::Enter | KeyCode::Char(' ') => {
                                if listings[index].is_dir {
                                    sub_dir = sub_dir.join(&listings[index].name);
                                    current_page = 1;
                                    current_selected_row = 0;
                                    break 'pages;
                                } else {
                                    if listings[index].is_media {
                                        let handle = call_play_video(listings[index].clone());

                                        execute!(stdout(), terminal::Clear(terminal::ClearType::All))?;
                                        execute!(stdout(), SavePosition, MoveTo(0,0))?;
                                        execute!(stdout(), Print(format!("Now playing {}\n", listings[index].name)))?;

                                        clear_input_buffer = true;
                                        handle.join().unwrap();
                                        break 'input;
                                    }
                                }
                            },
                            KeyCode::Esc => {
                                if sub_dir == anime_dir {
                                    break 'main; // Bye
                                } else {
                                    sub_dir.pop();
                                    current_page = 1;
                                    current_selected_row = 0;
                                    break 'pages;
                                }
                            },
                            KeyCode::Delete => {
                                if !delete_prompt {
                                    execute!(stdout(), terminal::Clear(terminal::ClearType::All))?;
                                    execute!(stdout(), MoveTo(0,0))?;
                                    execute!(stdout(), Print(format!("Are you sure you want to delete {}?\n", listings[index].name)))?;
                                    execute!(stdout(), Print(format!("Press delete to confirm, press any key to cancel\n")))?;
                                    execute!(stdout(), MoveTo(0,2))?;
                                    delete_prompt = true;
                                }
                            },
                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                show_empty_folders = !show_empty_folders;
                                current_page = 1;
                                current_selected_row = 0;
                                break 'pages;
                            },
                            _ => {},
                        }
                    },
                    _ => {}
                }

                if last_page != current_page {
                    if current_selected_row >= clp(current_page) { // If current cursor location is bigger than next page's max location
                        current_selected_row = clp(current_page) - 1;
                        execute!(stdout(), MoveTo(1, tip_cursor_offset + current_selected_row as u16))?;
                    }
                    execute!(stdout(), SavePosition)?;
                    break 'input; // Refresh pages
                }
                if !delete_prompt {
                    execute!(stdout(), MoveTo(1, tip_cursor_offset + current_selected_row as u16))?;
                }
            }
        }
    }
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn get_anime_listings(anime_dir: PathBuf, show_empty_folders: bool) -> Vec<AnimeListing> {
    let mut anime_list = vec![];
    for entry in anime_dir.read_dir().expect("Reading anime dir failed") {
        if let Ok(entry) = entry {
            if entry.path().is_dir() {
                let listing_episode_count = get_episode_count(entry.path());
                if listing_episode_count > 0 || show_empty_folders {
                    let listing = AnimeListing {
                        name: entry.path().file_name().and_then(OsStr::to_str).unwrap().to_owned(),
                        path: entry.path(),
                        is_dir: entry.path().is_dir(),
                        episode_count: listing_episode_count,
                        is_media: false,
                    };
                    anime_list.push(listing);
                }
            } else {
                match entry.path().extension().and_then(OsStr::to_str) {
                    Some(ext) => {
                        let is_media_file = is_valid_media_file(ext);
                        if is_media_file || show_empty_folders {
                            let episode = AnimeListing {
                                name: entry.path().file_name().and_then(OsStr::to_str).unwrap().to_owned(),
                                path: entry.path(),
                                is_dir: false,
                                episode_count: 0,
                                is_media: is_media_file,
                            };
                            anime_list.push(episode);
                        }
                    },
                    _ => {}
                }
            }
        }
    }

    anime_list
}

fn get_episode_count(folder_path: PathBuf) -> u32 {
    let mut num_episodes = 0;
    for entry in folder_path.read_dir().expect("Reading anime sub dir failed") {
        if let Ok(entry) = entry {
            if entry.path().is_file() {
                match entry.path().extension().and_then(OsStr::to_str) {
                    Some(ext) => {
                        if is_valid_media_file(ext) { num_episodes += 1; }
                    },
                    _ => {}
                }
            } else {
                num_episodes += get_episode_count(entry.path());
            }
        }
    }

    num_episodes
}

fn delete_item(listing: AnimeListing) -> std::io::Result<()> {
    //execute!(stdout(), Print(format!("Deleting {}\n", path.file_name().and_then(OsStr::to_str).unwrap())))?;
    if listing.is_dir {
        fs::remove_dir_all(listing.path)?;
    } else {
        fs::remove_file(listing.path)?;
    }
    Ok(())
}

fn call_play_video(episode: AnimeListing) -> thread::JoinHandle<()> {
    let mut filename = vec![];
    filename.push(episode.name);
    let dir_path = episode.path.parent().unwrap().to_path_buf();

    play_video(filename, dir_path)
}

fn call_play_videos(listings: Vec<AnimeListing>) -> thread::JoinHandle<()> {
    let mut episodes = vec![];
    for listing in listings {
        if !listing.is_dir && is_valid_media_file(listing.path.extension().and_then(OsStr::to_str).unwrap()) {
            episodes.push(listing);
        }
    }
    let filenames: Vec<String> = episodes.clone().into_iter().map(|episode| episode.name).collect();
    let dir_path = episodes[0].path.parent().unwrap().to_path_buf();

    play_video(filenames, dir_path)
}

#[cfg(feature = "mpv")]
pub fn play_video(filenames: Vec<String>, dir_path: PathBuf) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_secs(1));
        let mut i = 0;
        let mut timeout = 0;
        let mut filename = &filenames[i];
        let video_path = dir_path.join(filename);
        while timeout < 6 { //Initial connection waiting
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
                            let next_video_path = dir_path.join(filename);
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

#[cfg(not(feature = "mpv"))]
pub fn play_video(filenames: Vec<String>, dir_path: PathBuf) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_secs(1));
        let filename = &filenames[0];
        let video_path = dir_path.join(filename);

        let mut timeout = 0;
        while timeout < 6 { //Initial connection waiting
            if !video_path.is_file() {
                timeout += 1;
                thread::sleep(std::time::Duration::from_secs(5));
            } else {
                break;
            }
        }
        match opener::open(video_path) {
            Ok(_) => {},
            Err(e) => { eprintln!("{:?}", e)},
        };
    })
}