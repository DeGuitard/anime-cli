extern crate indicatif;
extern crate regex;

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, TcpStream};
use std::path::Path;
use std::str::from_utf8;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref DCC_SEND_REGEX: Regex =
        Regex::new(r#"DCC SEND (?:"([^"]+)"|(\S+)) (\d+) (\d+) (\d+)"#).unwrap();
    static ref DCC_ACCEPT_REGEX: Regex =
        Regex::new(r#"DCC ACCEPT (?:"([^"]+)"|(\S+)) (\d+) (\d+)"#).unwrap();
    static ref PING_REGEX: Regex = Regex::new(r#"PING :\d+"#).unwrap();
    static ref JOIN_REGEX: Regex = Regex::new(r#"JOIN :#.*"#).unwrap();
    // IRC numeric replies indicating server is ready for commands
    static ref MOTD_END_REGEX: Regex = Regex::new(r#":\S+ (376|422) "#).unwrap(); // RPL_ENDOFMOTD or ERR_NOMOTD
}

pub struct IRCRequest {
    pub server: String,
    pub channel: String,
    pub nickname: String,
    pub bot: String,
    pub packages: Vec<i32>,
}

struct DCCSend {
    filename: String,
    ip: IpAddr,
    port: String,
    file_size: usize,
    resume_position: usize,
}

pub fn connect_and_download(request: IRCRequest, shutdown: Arc<AtomicBool>, on_start: fn(String) -> ()) -> Result<(), String> {
    let mut download_handles = Vec::new();
    let mut has_joined = false;
    let mut server_ready = false; // Wait for MOTD end before joining
    let mp = MultiProgress::new();
    let mut pending_resumes: HashMap<String, DCCSend> = HashMap::new();
    let mut packages_requested = 0;

    // Show connection status
    let spinner = mp.add(ProgressBar::new_spinner());
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap()
    );
    spinner.enable_steady_tick(Duration::from_millis(100));
    spinner.set_message(format!("Connecting to {}...", request.server));

    let mut stream = log_in(&request).map_err(|e| {
        spinner.finish_and_clear();
        format!("Failed to connect: {}", e)
    })?;

    spinner.set_message(format!("Connected! Joining #{}...", request.channel));

    let mut message_buffer = String::new();
    let mut last_activity = std::time::Instant::now();
    let connection_timeout = Duration::from_secs(60);

    while download_handles.len() < request.packages.len() {
        // Check for shutdown signal
        if shutdown.load(Ordering::SeqCst) {
            spinner.finish_and_clear();
            eprintln!("\nInterrupted, cancelling downloads...");

            // Cancel any pending/in-progress XDCC transfers
            if packages_requested > download_handles.len() {
                stream.write_all(format!("PRIVMSG {} :xdcc cancel\r\n", request.bot).as_bytes()).ok();
            }

            stream.write_all("QUIT :Interrupted by user\r\n".as_bytes()).ok();
            stream.shutdown(Shutdown::Both).ok();
            return Err("Interrupted by user".to_string());
        }

        // Check for overall connection timeout
        if last_activity.elapsed() > connection_timeout {
            spinner.finish_and_clear();
            stream.write_all("QUIT :Connection timeout\r\n".as_bytes()).ok();
            stream.shutdown(Shutdown::Both).ok();
            return Err("Connection timed out waiting for server response. Please try again.".to_string());
        }

        let message = match read_next_message(&mut stream, &mut message_buffer) {
            Ok(msg) => {
                last_activity = std::time::Instant::now();
                msg
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    // Timeout after 1 second, continue to check shutdown flag
                    continue;
                }
                return Err(format!("Connection error: {}", e));
            }
        };

        // Check if server has completed welcome sequence
        if !server_ready && MOTD_END_REGEX.is_match(&message) {
            server_ready = true;
            spinner.set_message("Server ready, joining channel...");
        }

        // Always respond to PINGs
        if PING_REGEX.is_match(&message) {
            let pong = message.replace("PING", "PONG");
            stream.write_all(pong.as_bytes()).map_err(|e| format!("Failed to send PONG: {}", e))?;
        }

        // Join channel only after server is ready and we haven't joined yet
        if server_ready && !has_joined {
            let channel_join_cmd = format!("JOIN #{}\r\n", request.channel);
            stream.write_all(channel_join_cmd.as_bytes()).map_err(|e| format!("Failed to join channel: {}", e))?;
            has_joined = true;
            spinner.set_message(format!("Joining #{}...", request.channel));
        }
        if JOIN_REGEX.is_match(&message) {
            spinner.set_message(format!("Requesting {} package(s) from {}...", request.packages.len(), request.bot));
            for package in &request.packages {
                let xdcc_send_cmd = format!("PRIVMSG {} :xdcc send #{}\r\n", request.bot, package);
                stream.write_all(xdcc_send_cmd.as_bytes()).map_err(|e| format!("Failed to request package: {}", e))?;
                packages_requested += 1;
            }
        }
        if DCC_SEND_REGEX.is_match(&message) {
            let mut dcc_request = match parse_dcc_send(&message) {
                Some(req) => req,
                None => {
                    eprintln!("Warning: Failed to parse DCC SEND message");
                    continue;
                }
            };

            // Check if file exists and can be resumed
            if Path::new(&dcc_request.filename).exists() {
                let existing_size = std::fs::metadata(&dcc_request.filename)
                    .map(|m| m.len() as usize)
                    .unwrap_or(0);

                if existing_size > 0 && existing_size < dcc_request.file_size {
                    // File is partially downloaded, request resume
                    dcc_request.resume_position = existing_size;

                    // Quote filename if it contains spaces
                    let quoted_filename = if dcc_request.filename.contains(' ') {
                        format!("\"{}\"", dcc_request.filename)
                    } else {
                        dcc_request.filename.clone()
                    };

                    let resume_cmd = format!(
                        "PRIVMSG {} :\x01DCC RESUME {} {} {}\x01\r\n",
                        request.bot, quoted_filename, dcc_request.port, existing_size
                    );
                    stream.write_all(resume_cmd.as_bytes())
                        .map_err(|e| format!("Failed to send resume request: {}", e))?;

                    // Store the request and wait for ACCEPT
                    pending_resumes.insert(dcc_request.port.clone(), dcc_request);
                    spinner.set_message(format!("Requesting resume from {} bytes...", existing_size));
                    continue;
                } else if existing_size >= dcc_request.file_size {
                    // File is already complete, skip it
                    spinner.set_message(format!("File {} already complete, skipping", dcc_request.filename));
                    download_handles.push(thread::spawn(|| Ok(())));
                    continue;
                }
            }

            // New download or resume not needed
            start_download(dcc_request, &mp, &spinner, download_handles.is_empty(), shutdown.clone(), on_start, &mut download_handles);
        }
        if DCC_ACCEPT_REGEX.is_match(&message) {
            // Resume accepted, start download
            if let Some(port) = parse_dcc_accept_port(&message) {
                if let Some(dcc_request) = pending_resumes.remove(&port) {
                    start_download(dcc_request, &mp, &spinner, download_handles.is_empty(), shutdown.clone(), on_start, &mut download_handles);
                }
            } else {
                eprintln!("Warning: Failed to parse DCC ACCEPT message");
            }
        }
    }
    stream
        .write_all("QUIT :my job is done here!\r\n".as_bytes())
        .ok();
    stream.shutdown(Shutdown::Both).ok();

    // Wait for all downloads to complete
    // Download threads check shutdown flag themselves, so they'll exit cleanly if interrupted
    for handle in download_handles {
        match handle.join() {
            Ok(Ok(())) => {}, // Download succeeded
            Ok(Err(e)) => {
                eprintln!("Download error: {}", e);
            }
            Err(_) => {
                eprintln!("Download thread panicked");
            }
        }
    }

    Ok(())
}

fn log_in(request: &IRCRequest) -> Result<TcpStream, std::io::Error> {
    let mut stream = TcpStream::connect(&request.server)?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?; // Short timeout to check shutdown flag
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    stream.write_all(format!("NICK {}\r\n", request.nickname).as_bytes())?;
    stream.write_all(format!("USER {} 0 * {}\r\n", request.nickname, request.nickname).as_bytes())?;
    Ok(stream)
}

fn read_next_message(
    stream: &mut TcpStream,
    message_builder: &mut String,
) -> Result<String, std::io::Error> {
    let mut buffer = [0; 4];
    const MAX_MESSAGE_SIZE: usize = 4096; // Prevent unbounded growth

    while !message_builder.contains("\n") {
        // Prevent DoS from malformed messages
        if message_builder.len() > MAX_MESSAGE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Message too large"
            ));
        }

        match stream.read(&mut buffer[..]) {
            Ok(0) => {
                // EOF - connection closed
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Connection closed by server"
                ));
            }
            Ok(count) => {
                message_builder.push_str(from_utf8(&buffer[..count]).unwrap_or_default());
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                // EINTR - system call was interrupted by a signal, retry
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    // We know there's a newline because the loop condition ensures it
    let endline_offset = match message_builder.find('\n') {
        Some(pos) => pos + 1,
        None => return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Internal error: newline not found in message buffer"
        )),
    };

    let message = message_builder.get(..endline_offset)
        .ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Internal error: invalid message buffer range"
        ))?
        .to_string();

    message_builder.replace_range(..endline_offset, "");
    Ok(message)
}

fn parse_dcc_send(message: &str) -> Option<DCCSend> {
    let captures = DCC_SEND_REGEX.captures(message)?;
    // Filename can be in capture group 1 (quoted) or 2 (unquoted)
    let filename = captures.get(1)
        .or_else(|| captures.get(2))
        .map(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let ip_number = captures[3].parse::<u32>().ok()?;
    let file_size = captures[5].parse::<usize>().ok()?;

    Some(DCCSend {
        filename,
        ip: IpAddr::V4(Ipv4Addr::from(ip_number)),
        port: captures[4].to_string(),
        file_size,
        resume_position: 0,
    })
}

fn parse_dcc_accept_port(message: &str) -> Option<String> {
    let captures = DCC_ACCEPT_REGEX.captures(message)?;
    Some(captures[3].to_string())
}

fn start_download(
    dcc_request: DCCSend,
    mp: &MultiProgress,
    spinner: &ProgressBar,
    is_first: bool,
    shutdown: Arc<AtomicBool>,
    on_start: fn(String) -> (),
    download_handles: &mut Vec<std::thread::JoinHandle<std::result::Result<(), std::io::Error>>>,
) {
    // Clear the spinner once we start downloading
    if is_first {
        spinner.finish_and_clear();
    }

    let pb = mp.add(ProgressBar::new(dcc_request.file_size as u64));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({percent}%) {bytes_per_sec} ETA: {eta}")
            .unwrap()
            .progress_chars("#>-")
    );

    let action = if dcc_request.resume_position > 0 {
        "Resuming"
    } else {
        "Downloading"
    };
    pb.set_message(format!("{} {}", action, dcc_request.filename));
    pb.enable_steady_tick(Duration::from_millis(500));

    let handle = thread::spawn(move || {
        download_file(dcc_request, pb, shutdown, on_start)
    });
    download_handles.push(handle);
}

fn download_file(
    request: DCCSend,
    progress_bar: ProgressBar,
    shutdown: Arc<AtomicBool>,
    on_start: fn(String) -> (),
) -> std::result::Result<(), std::io::Error> {
    let filename = request.filename.to_string();

    // Open file in append mode if resuming, otherwise create new
    let mut file = if request.resume_position > 0 {
        OpenOptions::new()
            .append(true)
            .open(&request.filename)?
    } else {
        File::create(&request.filename)?
    };

    let mut stream = TcpStream::connect(format!("{}:{}", request.ip, request.port))?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?; // Short timeout to check shutdown flag
    let mut buffer = [0; 4096];
    let mut progress: usize = request.resume_position;

    // Set initial progress bar position for resume
    if request.resume_position > 0 {
        progress_bar.set_position(request.resume_position as u64);
    }

    on_start(filename);

    while progress < request.file_size {
        // Check for shutdown signal
        if shutdown.load(Ordering::SeqCst) {
            progress_bar.set_message(format!("✗ Interrupted {}", request.filename));
            progress_bar.abandon();
            stream.shutdown(Shutdown::Both).ok();
            file.flush()?;
            return Ok(());
        }

        match stream.read(&mut buffer[..]) {
            Ok(count) if count > 0 => {
                file.write_all(&buffer[..count])?;
                progress += count;
                progress_bar.set_position(progress as u64);
            }
            Ok(_) => break, // EOF
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout, continue to check shutdown flag
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    progress_bar.finish_with_message(format!("✓ Downloaded {}", request.filename));
    stream.shutdown(Shutdown::Both)?;
    file.flush()?;
    Ok(())
}
