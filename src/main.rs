use std::env;
use std::fs::File;
use std::io::{self, BufRead};
use std::io::{Error, ErrorKind};

use serde_json::Result;
use serde_json::Error as SerdeJsonError;
use regex::Regex;
use reqwest;
use reqwest::Error as ReqwestError;
use urlencoding::encode;


#[tokio::main]
async fn main() {
    // Get the command-line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <text_file_path>", args[0]);
        std::process::exit(1);
    }

    let text_file_path = &args[1];

    match get_card_image("Tayam, Luminous Enigma", "C20").await {
        Ok(image_url) => println!("{}", image_url),
        Err(e) => eprintln!("Error: {}", e),
    }

    match parse_text_file(text_file_path).await {
        Ok(card_data) => {
            for (card_name, set_name) in card_data {
            
                match get_card_image(&card_name, &set_name).await {
                    Ok(image_url) => println!("{}", image_url),
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}

async fn get_card_image(card_name: &str, set_name: &str) -> Result<String> {
    // URL encoding for card_name and set_name
    let card_name = encode(card_name);
    let set_name = encode(set_name);

    let url = format!(
        "https://api.scryfall.com/cards/named?fuzzy={}&set={}",
        card_name, set_name
    );

    let res = reqwest::get(&url).await.unwrap();

    if res.status().is_success() {
        let data: serde_json::Value = res.json().await.unwrap();
        if let Some(image_uris) = data["image_uris"].as_object() {
            if let Some(png_url) = image_uris.get("png") {
                return Ok(png_url.as_str().unwrap().to_string());
            } else {
                panic!("Error: Image URL not found in the JSON response.");
                //Err(ReqwestError)
            }
        } else {
            panic!("jndkj")
        }
    } else {
        panic!("Error: Failed to retrieve card data. Status Code: {}", res.status());
        //Err(SerdeJsonError)
    }
}


async fn parse_text_file(txt_path: &str) -> io::Result<Vec<(String, String)>> {
    let mut card_names = Vec::new();
    let mut set_names = Vec::new();
    let card_pattern = Regex::new(r"\d (.*) \(").unwrap();
    let set_pattern = Regex::new(r"\(([a-zA-Z0-9]*)\)").unwrap();

    let file = File::open(txt_path)?;

    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if let (Some(card_match), Some(set_match)) = (card_pattern.captures(&line), set_pattern.captures(&line)) {
            card_names.push(card_match[1].to_string());
            set_names.push(set_match[1].to_string());
        } else {
            // Handle lines that don't match the expected format
            eprintln!("Warning: Skipped line - {}", line.trim());
        }
    }

    Ok(card_names.into_iter().zip(set_names).collect())
}
