extern crate reqwest;
extern crate serde;

use reqwest::Error;
use serde::Deserialize;
use std::result::Result;

const API_URL: &str = "https://api.nibl.co.uk/nibl";

#[derive(Clone)]
pub struct DCCPackage {
    pub number: i32,
    pub bot: String,
    pub filename: String,
    pub sizekbits: i64,
}

pub fn find_package(query: &String, episode: &Option<u16>) -> Result<DCCPackage, String> {
    let packages = match search_packages(query, episode) {
        Ok(p) => p,
        Err(e) => return Err(format!("Error while fetching results: {}", e)),
    };

    let first_package = match packages.first() {
        Some(p) => p,
        None => return Err("Could not find any result for this query.".to_string()),
    };

    let bot_name = match find_bot_name(&first_package.bot_id) {
        Some(b) => b,
        None => return Err("Results found, but unknown bot.".to_string()),
    };

    Ok(DCCPackage {
        bot: bot_name.to_string(),
        number: first_package.number,
        filename: first_package.name.clone(),
        sizekbits: first_package.sizekbits,
    })
}

fn search_packages(query: &String, episode: &Option<u16>) -> Result<Vec<Package>, Error> {
    let mut search_url = format!("{}/search?query={}", API_URL, query);
    if episode.is_some() {
        search_url += &format!("&episodeNumber={}", episode.unwrap());
    }
    let mut response = reqwest::get(&search_url)?;
    let search_result: SearchResult = response.json()?;
    if search_result.status != "OK" {
        panic!("Could not search package: {}", search_result.message);
    }
    Ok(search_result.content)
}

fn find_bot_name(id: &i64) -> Option<String> {
    let bot_list = get_bot_list();
    let bot = bot_list.iter().find(|bot| &bot.id == id);
    match bot {
        Some(b) => Some(b.name.to_string()),
        None => None,
    }
}

fn get_bot_list() -> Vec<Bot> {
    let mut response =
        reqwest::get(&format!("{}/bots", API_URL)).expect("Could not fetch bot list");
    let bot_list: BotList = response.json().expect("Could not parse bot list.");
    if bot_list.status != "OK" {
        panic!("Could not fetch bot list: {}", bot_list.message);
    }
    bot_list.content
}

#[derive(Deserialize)]
struct BotList {
    status: String,
    message: String,
    content: Vec<Bot>,
}

#[derive(Deserialize)]
struct Bot {
    id: i64,
    name: String,
}

#[derive(Deserialize)]
struct SearchResult {
    status: String,
    message: String,
    content: Vec<Package>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Package {
    bot_id: i64,
    number: i32,
    name: String,
    _size: String,
    sizekbits: i64,
}
