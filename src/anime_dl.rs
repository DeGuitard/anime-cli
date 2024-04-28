extern crate pbr;
extern crate regex;

use std::fs::File;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, TcpStream};
use std::str::from_utf8;
use std::thread;

use lazy_static::lazy_static;
use pbr::{MultiBar, Pipe, ProgressBar, Units};
use regex::Regex;

lazy_static! {
    static ref DCC_SEND_REGEX: Regex =
        Regex::new(r#"DCC SEND "?(.*)"? (\d+) (\d+) (\d+)"#).unwrap();
    static ref PING_REGEX: Regex = Regex::new(r#"PING :.*"#).unwrap();
    static ref JOIN_REGEX: Regex = Regex::new(r#"JOIN :#.*"#).unwrap();
    static ref MODE_REGEX: Regex = Regex::new(r#"MODE .* :\+.*"#).unwrap();
    //static ref NOTICE_REGEX: Regex = Regex::new(r#"NOTICE .* You already requested"#).unwrap();
}

pub struct IRCRequest {
    pub server: String,
    pub channel: String,
    pub nickname: String,
    pub bot: Vec<String>,
    pub packages: Vec<String>,
}

struct DCCSend {
    filename: String,
    ip: IpAddr,
    port: String,
    file_size: usize,
}

pub fn connect_and_download(request: IRCRequest, on_start: fn(String) -> thread::JoinHandle<()>) -> Result<(), String> {
    let mut download_handles = Vec::new();
    let mut has_joined = false;
    let mut multi_bar = MultiBar::new();
    let mut stream = log_in(&request).unwrap();

    let mut message_buffer = String::new();
    while download_handles.len() < request.packages.len() {
        let message = read_next_message(&mut stream, &mut message_buffer).unwrap();
        //println!("{}", message);
        if PING_REGEX.is_match(&message) {
            let pong = message.replace("PING", "PONG");
            stream.write(pong.as_bytes()).unwrap();
            if !has_joined {
                let channel_join_cmd = format!("JOIN #{}\r\n", request.channel);
                stream.write(channel_join_cmd.as_bytes()).unwrap();
            }
        }
        if JOIN_REGEX.is_match(&message) {
            has_joined = true;
            let mut i = 0;
            for package in &request.packages {
                let xdcc_send_cmd =
                    format!("PRIVMSG {} :xdcc send #{}\r\n", &request.bot[i], package);
                stream.write(xdcc_send_cmd.as_bytes()).unwrap();
                i += 1;
            }
        }
        if MODE_REGEX.is_match(&message) {
            if !has_joined {
                let channel_join_cmd = format!("JOIN #{}\r\n", request.channel);
                stream.write(channel_join_cmd.as_bytes()).unwrap();
            }
        }
        if DCC_SEND_REGEX.is_match(&message) {
            let request = parse_dcc_send(&message);
            let mut progress_bar = multi_bar.create_bar(request.file_size as u64);
            let handle = thread::spawn(move || {
                download_file(request, &mut progress_bar, on_start).unwrap();
            });
            download_handles.push(handle);
        }
    }
    stream
        .write("QUIT :my job is done here!\r\n".as_bytes())
        .unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
    multi_bar.listen();
    download_handles
        .into_iter()
        .for_each(|handle| handle.join().unwrap());
    Ok(())
}

fn log_in(request: &IRCRequest) -> Result<TcpStream, std::io::Error> {
    let mut stream = TcpStream::connect(&request.server)?;
    stream.write(format!("NICK {}\r\n", request.nickname).as_bytes())?;
    stream.write(format!("USER {} 0 * {}\r\n", request.nickname, request.nickname).as_bytes())?;
    Ok(stream)
}

fn read_next_message(
    stream: &mut TcpStream,
    message_builder: &mut String,
) -> Result<String, std::io::Error> {
    let mut buffer = [0; 4];
    while !message_builder.contains("\n") {
        let count = stream.read(&mut buffer[..])?;
        message_builder.push_str(from_utf8(&buffer[..count]).unwrap_or_default());
    }
    let endline_offset = message_builder.find('\n').unwrap() + 1;
    let message = message_builder.get(..endline_offset).unwrap().to_string();
    message_builder.replace_range(..endline_offset, "");
    Ok(message)
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
    progress_bar: &mut ProgressBar<Pipe>,
    on_start: fn(String) -> thread::JoinHandle<()>,
) -> std::result::Result<(), std::io::Error> {
    let filename = request.filename.to_string();
    let mut file = File::create(&request.filename)?;
    let mut stream = TcpStream::connect(format!("{}:{}", request.ip, request.port))?;
    let mut buffer = [0; 4096];
    let mut progress: usize = 0;
    progress_bar.set_units(Units::Bytes);
    progress_bar.message(&format!("{}: ", &request.filename));
    let videohandle = on_start(filename);

    while progress < request.file_size {
        let count = stream.read(&mut buffer[..])?;
        file.write(&mut buffer[..count])?;
        progress += count;
        progress_bar.set(progress as u64);
    }
    progress_bar.finish();
    stream.shutdown(Shutdown::Both)?;
    file.flush()?;

    videohandle.join().unwrap();
    Ok(())
}
