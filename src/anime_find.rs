extern crate reqwest;
extern crate serde;
extern crate serde_json;

use reqwest::Error;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::result::Result;

const API_URL: &str = "https://api.nibl.co.uk/nibl";

pub struct DCCPackage {
    pub number: i32,
    pub bot: String,
}

pub fn find_packages(query: &String, episode: &Vec<u16>) -> Result<Vec<DCCPackage>, String> {
    let mut packages = Vec::new();
    for &ep in episode {
        match find_package(query, &Some(ep)) {
            Ok(pkg) => packages.push(pkg),
            Err(e) => return Err(format!("Episode {}: {}", ep, e)),
        }
    }
    Ok(packages)
}

pub fn find_package(query: &String, episode: &Option<u16>) -> Result<DCCPackage, String> {
    let packages = match search_packages(query, episode) {
        Ok(p) => p,
        Err(_) => {
            return Err(format!(
                "Failed to search for '{}'. Please check your internet connection and try again.",
                query
            ))
        }
    };

    let first_package = match packages.first() {
        Some(p) => p,
        None => {
            let msg = if let Some(ep) = episode {
                format!(
                    "No results found for '{}' episode {}. The episode may not exist or may not be available yet.",
                    query, ep
                )
            } else {
                format!("No results found for '{}'. Please check the title and try again.", query)
            };
            return Err(msg);
        }
    };

    let bot_name = match find_bot_name(&first_package.bot_id) {
        Some(b) => b,
        None => {
            return Err(format!(
                "Found results for '{}' but the download bot is not available. Please try again later.",
                query
            ))
        }
    };

    Ok(DCCPackage {
        bot: bot_name.to_string(),
        number: first_package.number,
    })
}

fn search_packages(query: &str, episode: &Option<u16>) -> Result<Vec<Package>, Error> {
    // URL encode the query to handle special characters safely
    let encoded_query = urlencoding::encode(query);
    let mut search_url = format!("{}/search?query={}", API_URL, encoded_query);
    if let Some(ep) = episode {
        search_url += &format!("&episodeNumber={}", ep);
    }

    // Create client with timeout to prevent hanging
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut response = client.get(&search_url).send()?;
    let search_result: SearchResult = response.json()?;
    // Note: API errors are handled by returning an empty content array, not by status field
    Ok(search_result.content)
}

fn find_bot_name(id: &i64) -> Option<String> {
    let bot_list = match get_bot_list() {
        Ok(list) => list,
        Err(_) => return None,
    };
    let bot = bot_list.iter().find(|bot| &bot.id == id);
    bot.map(|b| b.name.to_string())
}

fn get_bot_list() -> Result<Vec<Bot>, String> {
    let cache_path = get_cache_path();
    match File::open(&cache_path) {
        Ok(file) => {
            let reader = BufReader::new(file);
            match serde_json::de::from_reader(reader) {
                Ok(list) => Ok(list),
                Err(_) => {
                    // Cache is corrupted, try to fetch fresh data
                    fetch_and_cache_bot_list(&cache_path)
                }
            }
        }
        Err(_) => {
            // Cache doesn't exist, fetch fresh data
            fetch_and_cache_bot_list(&cache_path)
        }
    }
}

fn get_cache_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("animecli.botlist.json");
    path
}

fn fetch_and_cache_bot_list(path: &Path) -> Result<Vec<Bot>, String> {
    // Create client with timeout
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Err("Failed to create HTTP client".to_string()),
    };

    let mut response = match client.get(&format!("{}/bots", API_URL)).send() {
        Ok(r) => r,
        Err(_) => {
            return Err(
                "Failed to fetch bot list from server. Please check your internet connection."
                    .to_string(),
            )
        }
    };

    let bot_list: BotList = match response.json() {
        Ok(bl) => bl,
        Err(_) => {
            return Err("Failed to parse bot list from server. Please try again later.".to_string())
        }
    };

    if bot_list.status != "OK" {
        return Err(format!(
            "Server returned an error: {}. Please try again later.",
            bot_list.message
        ));
    }

    // Cache the bot list (ignore errors as cache is optional)
    let _ = save_bot_list_to_cache(&bot_list, path);

    Ok(bot_list.content)
}

fn save_bot_list_to_cache(bot_list: &BotList, path: &Path) -> Result<(), String> {
    let json_string = match serde_json::to_string(&bot_list.content) {
        Ok(s) => s,
        Err(e) => return Err(format!("Could not serialize bot list: {}", e)),
    };
    match File::create(path) {
        Ok(mut file) => match file.write_all(json_string.as_bytes()) {
            Ok(_) => Ok(()),
            Err(why) => Err(format!("Could not write bot list to cache: {}", why)),
        },
        Err(why) => Err(format!("Could not create cache file: {}", why)),
    }
}

#[derive(Deserialize)]
struct BotList {
    status: String,
    message: String,
    content: Vec<Bot>,
}

#[derive(Deserialize, Serialize)]
struct Bot {
    id: i64,
    name: String,
}

#[derive(Deserialize)]
struct SearchResult {
    content: Vec<Package>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Package {
    bot_id: i64,
    number: i32,
}
