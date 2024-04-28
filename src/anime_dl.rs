extern crate pbr;
extern crate regex;
extern crate rand;

use std::io::{Read, Write, Error, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Shutdown, TcpStream};
use std::str::from_utf8;
use std::{thread, time, fs};
use std::path::{PathBuf};
use rand::Rng;

use lazy_static::lazy_static;
use std::sync::mpsc::{Sender};
use regex::Regex;
use std::thread::sleep;

lazy_static! {
    //static ref NICKNAME_REGEX: Regex = Regex::new(r#""#).unwrap();
    static ref DCC_SEND_REGEX: Regex =
        Regex::new(r#"DCC SEND "?(.*)"? (\d+) (\d+) (\d+)"#).unwrap();
    static ref PING_REGEX: Regex = Regex::new(r#"PING :.*"#).unwrap();
    static ref JOIN_REGEX: Regex = Regex::new(r#"JOIN :#.*"#).unwrap();
    static ref MODE_REGEX: Regex = Regex::new(r#"MODE .* :\+.*"#).unwrap();
    static ref NOTICE_REGEX: Regex = Regex::new(r#"NOTICE .* You already requested"#).unwrap();
    static ref QUEUE_REGEX: Regex = Regex::new(r#".* queued too many .*"#).unwrap();
    static ref RESUME_REGEX: Regex = Regex::new(r#"DCC ACCEPT .*"#).unwrap();
}

pub struct IRCRequest {
    pub server: String,
    pub channel: String,
    pub nickname: String,
    pub bot: Vec<String>,
    pub packages: Vec<String>,
}

#[derive(Clone)]
struct DCCSend {
    filename: String,
    ip: IpAddr,
    port: String,
    file_size: usize,
}

struct IRCConnection {
    socket: TcpStream,
    partial_msg: String,
}

impl IRCConnection {
    fn read_message(&mut self) -> Option<String> {
        let mut buffer = [0; 4];
        let count = match self.socket.read(&mut buffer[..]) {
            Ok(a) => a,
            Err(_) => return Some(String::from("Error"))
        };
        self.partial_msg.push_str(from_utf8(&buffer[..count]).unwrap_or_default());
        //println!("{}", self.message_builder);
        if self.partial_msg.contains('\n') {
            let endline_offset = self.partial_msg.find('\n').unwrap() + 1;
            let message = self.partial_msg.get(..endline_offset).unwrap().to_string();
            self.partial_msg.replace_range(..endline_offset, "");
            Some(message)
        } else {
            None
        }
    }
}

pub fn connect_and_download(request: IRCRequest, channel_senders: Vec<Sender<i64>>, status_bar_sender: Sender<String>, dir_path: PathBuf) -> Result<(), std::io::Error> {
    status_bar_sender.send(format!("Connecting to Rizon...")).unwrap();

    let mut download_handles = Vec::new();
    let mut has_joined = false;
    let stream = log_in(&request).unwrap();
    let mut connection : IRCConnection = IRCConnection { socket: stream, partial_msg: "".to_string()};

    let mut next = time::Instant::now() + time::Duration::from_millis(500);
    let timeout_threshold = 5;
    let mut timeout_counter = 0;
    status_bar_sender.send(format!("Logging into Rizon...")).unwrap();
    while !has_joined {
        let message = connection.read_message();
        let now = time::Instant::now();
        if message.is_some() {
            let msg = &message.unwrap();
            //println!("{}",msg);
            if "Error".eq(msg) {
                return Err(Error::new(ErrorKind::Other, String::from("Error reading TcpStream")))
            }
            if PING_REGEX.is_match(msg) {
                let pong = msg.replace("PING", "PONG");
                connection.socket.write(pong.as_bytes()).unwrap();
                if !has_joined {
                    let channel_join_cmd = format!("JOIN #{}\r\n", request.channel);
                    connection.socket.write(channel_join_cmd.as_bytes()).unwrap();
                }
            }
            if MODE_REGEX.is_match(msg) {
                if !has_joined {
                    let channel_join_cmd = format!("JOIN #{}\r\n", request.channel);
                    connection.socket.write(channel_join_cmd.as_bytes()).unwrap();
                }
            }
            if JOIN_REGEX.is_match(msg) {
                has_joined = true;
            }
        } else {
            if now >= next {
                let channel_join_cmd = format!("JOIN #{}\r\n", request.channel);
                connection.socket.write(channel_join_cmd.as_bytes()).unwrap();
                next = now + time::Duration::from_millis(500);
                timeout_counter += 1;
                if timeout_counter > timeout_threshold {
                    return Err(Error::new(ErrorKind::Other, String::from("Timed out logging in")))
                }

            }
        }
        //thread::sleep(time::Duration::from_micros(10));
    }

    status_bar_sender.send(format!("Connected")).unwrap();

    let mut i = 0;
    let mut requests : Vec<DCCSend> = vec![];
    let mut resume = false;
    let mut wait = false;
    let mut received_reply;
    while download_handles.len() < request.packages.len() && timeout_counter <= timeout_threshold {
        if wait {
            //wait til a previous package is downloaded then proceed
            let f = fs::File::open(&requests[i-1].filename)?;
            let meta = f.metadata()?;
            while meta.len() < requests[i-1].file_size as u64 {
                sleep(time::Duration::from_secs(1));
            }
            wait = false;
        }
        let package_bot = &request.bot[i];
        let package_number = &request.packages[i];
        if !resume {
            let xdcc_send_cmd =
                format!("PRIVMSG {} :xdcc send #{}\r\n", package_bot, package_number);
            connection.socket.write(xdcc_send_cmd.as_bytes()).unwrap();
        }

        next = time::Instant::now() + time::Duration::from_millis(3000);
        timeout_counter = 0;
        received_reply = false;
        while !received_reply && timeout_counter <= timeout_threshold {
            let message = connection.read_message();
            let now = time::Instant::now();
            if message.is_some() {
                let msg = &message.unwrap();
                //println!("{}",msg);
                if "Error".eq(msg) {
                    return Err(Error::new(ErrorKind::Other, String::from(format!("Error reading TcpStream on pack {}", package_number))))
                }
                if DCC_SEND_REGEX.is_match(msg) {
                    let request = parse_dcc_send(msg);
                    requests.push(request);
                    status_bar_sender.send(format!("Now downloading {}", &requests[i].filename)).unwrap();
                    if std::path::Path::new(&requests[i].filename).exists() {
                        status_bar_sender.send(format!("Found an existing {}", &requests[i].filename)).unwrap();
                        let f = fs::File::open(&requests[i].filename)?;
                        let meta = f.metadata()?;
                        if (meta.len() as usize) < requests[i].file_size {
                            let xdcc_resume_cmd =
                                format!("PRIVMSG {} :\x01DCC RESUME \"{}\" {} {}\x01\r\n", package_bot, &requests[i].filename, &requests[i].port, meta.len());
                            connection.socket.write(xdcc_resume_cmd.as_bytes()).unwrap();
                            resume = true;
                        }
                    }
                    if !resume {
                        //let mut progress_bar = multi_bar.create_bar(requests[i].file_size as u64);
                        let req = requests[i].clone();
                        let sender = channel_senders[i].clone();
                        let path = dir_path.clone();
                        let handle = thread::spawn(move || {
                            download_file(req, sender, path).unwrap();
                        });
                        download_handles.push(handle);
                        i += 1;
                    }
                    received_reply = true;
                }
                if resume && RESUME_REGEX.is_match(msg){
                    status_bar_sender.send(format!("Attempting to resume download for {}", requests[i].filename)).unwrap();
                    //let mut progress_bar = multi_bar.create_bar(requests[i].file_size as u64);
                    let req = requests[i].clone();
                    let sender = channel_senders[i].clone();
                    let path = dir_path.clone();
                    let handle = thread::spawn(move || {
                        download_file(req, sender, path).unwrap();
                    });
                    download_handles.push(handle);
                    i += 1;
                    resume = false;
                    received_reply = true;
                }
                if QUEUE_REGEX.is_match(msg) {
                    //bot tells you that you can't queue up a new file
                    wait = true;
                    received_reply = true;
                }
                if NOTICE_REGEX.is_match(msg) {
                    status_bar_sender.send(format!("A previous request was made for pack {}, attempting to cancel and retry", package_number)).unwrap();
                    let xdcc_remove_cmd =
                        format!("PRIVMSG {} :xdcc remove #{}\r\n", package_bot, package_number);
                    connection.socket.write(xdcc_remove_cmd.as_bytes()).unwrap();
                    let xdcc_cancel_cmd =
                        format!("PRIVMSG {} :\x01XDCC CANCEL\x01\r\n", package_bot);
                    connection.socket.write(xdcc_cancel_cmd.as_bytes()).unwrap();
                    received_reply = true;
                }
            } else {
                //postpone the timeout if currently downloading, if bot doesn't care to give queue message
                //some batch xdcc bots will add you into a queue but won't send more than x number of dcc sends
                let mut dl_in_progress = false;
                if (i > requests.len()) && std::path::Path::new(&requests[i-1].filename).exists() {
                    let f = fs::File::open(&requests[i - 1].filename)?;
                    let meta = f.metadata()?;

                    if !meta.len() < requests[i - 1].file_size as u64 {
                        dl_in_progress = true;
                    }
                }
                if now >= next && !dl_in_progress {
                    next = now + time::Duration::from_millis(3000);
                    timeout_counter += 1;
                    status_bar_sender.send(format!("({}/{}) Waiting on dcc send reply for pack {}...", timeout_counter, timeout_threshold, package_number)).unwrap();
                    if timeout_counter > timeout_threshold {
                        status_bar_sender.send(format!("Timed out receiving dcc send for pack {}", package_number)).unwrap();
                    }
                }
            }
        }
    }

    connection.socket
        .write("QUIT :my job is done here!\r\n".as_bytes())
        .unwrap();
    connection.socket.shutdown(Shutdown::Both).unwrap();
    download_handles
        .into_iter()
        .for_each(|handle| handle.join().unwrap());
    status_bar_sender.send("Success".to_string()).unwrap();
    Ok(())
}

fn log_in(request: &IRCRequest) -> Result<TcpStream, std::io::Error> {
    let mut stream = TcpStream::connect(&request.server)?;
    let mut rng = rand::thread_rng();
    let rng_num: u16 = rng.gen();
    let rng_nick = format!("{}{}", request.nickname, rng_num);
    stream.write(format!("NICK {}\r\n", rng_nick).as_bytes())?;
    stream.write(format!("USER {} 0 * {}\r\n", rng_nick, rng_nick).as_bytes())?;
    Ok(stream)
}

fn parse_dcc_send(message: &String) -> DCCSend {
    let captures = DCC_SEND_REGEX.captures(&message).unwrap();
    let ip_number = captures[2].parse::<u32>().unwrap();
    DCCSend {
        filename: captures[1].to_string().replace("\"",""),
        ip: IpAddr::V4(Ipv4Addr::from(ip_number)),
        port: captures[3].to_string(),
        file_size: captures[4].parse::<usize>().unwrap(),
    }
}

fn download_file(
    request: DCCSend,
    sender: Sender<i64>,
    dir_path: PathBuf) -> std::result::Result<(), std::io::Error> {
    let file_path = dir_path.join(&request.filename);
    let mut file =  match fs::OpenOptions::new().append(true).open(file_path.clone()) {
        Ok(existing_file) => existing_file,
        Err(_) => fs::File::create(file_path.clone())?
    };
    let mut stream = TcpStream::connect(format!("{}:{}", request.ip, request.port))?;
    let mut buffer = [0; 4096];
    let meta = file.metadata()?;
    let mut progress = meta.len() as usize;

    while progress < request.file_size {
        let count = stream.read(&mut buffer[..])?;
        file.write(&mut buffer[..count])?;
        progress += count;
        sender.send(progress as i64).unwrap();
    }

    sender.send(-1).unwrap();
    stream.shutdown(Shutdown::Both)?;
    file.flush()?;

    Ok(())
}
