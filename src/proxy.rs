extern crate image;
extern crate printpdf;

use std::env::consts::OS;
use std::fs::File;
use std::io::{self, BufRead, BufWriter, Cursor};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use futures::future::try_join_all;
use image::codecs::png::PngDecoder;
use image::io::Reader as ImageReader;
use printpdf::*;
use regex::Regex;
use reqwest::{self, Client};
use tokio::time::{sleep, Duration};
use urlencoding::encode;

const CARDBACK_IMAGE: &[u8] = include_bytes!("../image/magic_card_back.png");
const PAGE_X: f64 = 210.0;
const PAGE_Y: f64 = 297.0;
// Constants for card size and grid layout
const CARD_WIDTH_MM: f64 = 63.0;
const CARD_HEIGHT_MM: f64 = 88.0;
const GRID_COLS: usize = 3;
const GRID_ROWS: usize = 3;

#[derive(Debug)]
struct CardImageUrls {
    front: Option<String>,
    back: Option<String>,
}

impl IntoIterator for CardImageUrls {
    type Item = Option<String>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        vec![self.front, self.back].into_iter()
    }
}

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[tokio::main]
pub async fn run(file_path: Option<PathBuf>, grid: bool, padding_length: f64) {
    let selected_file = match file_path {
        None => {
            eprintln!("Please select a .txt file including the decklist.");
            return;
        }
        Some(file_path) => file_path,
    };

    let start: std::time::Instant = std::time::Instant::now();

    let text_file_path = match selected_file.into_os_string().into_string() {
        Ok(text_file_path) => text_file_path,
        Err(e) => {
            eprint!("There was an error parsing the file path: {:?}", e);
            return;
        }
    };

    let file = match File::open(&text_file_path) {
        Ok(file) => file,
        Err(e) => {
            eprint!("There was an error opening the file: {:?}", e);
            return;
        }
    };

    let card_data = match parse_text_file(file).await {
        Ok(card_data) => card_data,
        Err(e) => {
            eprintln!("Error parsing the text file: {}", e);
            return;
        }
    };

    let mut image_futures = vec![];
    let mut requests_count: i32 = 0;
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .build()
        .unwrap();
    let cards_per_page = GRID_COLS * GRID_ROWS;

    for (card_name, set_name) in card_data {
        match get_card_image_url(&client, &card_name, set_name.as_deref(), "png").await {
            Ok(card_image) => {
                if grid {
                    let image_future = tokio::spawn(get_card_image(card_image.front));
                    image_futures.push(image_future);
                } else {
                    for image_url in card_image {
                        let image_future = tokio::spawn(get_card_image(image_url));
                        image_futures.push(image_future);
                    }
                }

                println!("Downloading image for card {}", card_name);

                requests_count += 1;
                sleep(Duration::from_millis(50)).await;
            }
            Err(e) => {
                eprintln!("Error retrieving png url for card: {} => {}", card_name, e);
            }
        }
    }

    let images: Vec<std::result::Result<Image, anyhow::Error>> =
        try_join_all(image_futures).await.unwrap();

    if grid {
        create_pdf_grid(&text_file_path, images, cards_per_page, padding_length);
    } else {
        create_pdf_single(&text_file_path, images);
    }

    println!("Total number of scryfall requests: {}", requests_count);
    println!("Total processing time: {:.2?}", start.elapsed());
}

fn create_pdf_grid(
    text_file_path: &str,
    images: Vec<std::result::Result<Image, anyhow::Error>>,
    cards_per_page: usize,
    padding_length: f64,
) {
    let (doc, mut page, mut layer) =
        PdfDocument::new("PDF_Document_title", Mm(PAGE_X), Mm(PAGE_Y), "Layer 1");
    for (i, image) in images.into_iter().enumerate() {
        match image {
            Ok(image) => {
                if (i % cards_per_page == 0) & (i > 0) {
                    let (new_page, new_layer) = doc.add_page(Mm(PAGE_X), Mm(PAGE_Y), "new_layer");
                    page = new_page;
                    layer = new_layer;
                }
                let current_layer_ref = doc.get_page(page).get_layer(layer);
                // Column and row position in grid
                let col = i % GRID_COLS;
                let row = (i / GRID_COLS) % GRID_ROWS;
                // Calculate spacing to center grid on the page
                let total_grid_width =
                    CARD_WIDTH_MM * GRID_COLS as f64 + (GRID_COLS as f64 - 1.0) * padding_length;
                let total_grid_height = (CARD_HEIGHT_MM + padding_length) * GRID_ROWS as f64
                    + (GRID_ROWS as f64 - 1.0) * padding_length;
                let x_offset = (PAGE_X - total_grid_width) / 2.0;
                let y_offset = (PAGE_Y - total_grid_height) / 2.0;
                // Calculate position
                let x = Mm(x_offset + (CARD_WIDTH_MM + padding_length) * col as f64);
                let y =
                    Mm(PAGE_Y - y_offset - (CARD_HEIGHT_MM + padding_length) * (row as f64 + 1.0));
                image.add_to_layer(
                    current_layer_ref,
                    ImageTransform {
                        translate_x: Some(x),
                        translate_y: Some(y),
                        ..Default::default()
                    },
                );
            }
            Err(e) => eprintln!("Error getting image: {}", e),
        }
    }
    match save_pdf(text_file_path, doc) {
        Ok(pdf_filepath) => {
            println!("Saving pdf to path: {}", pdf_filepath);
            open_file_in_explorer(&pdf_filepath);
        }
        Err(e) => eprintln!("Error saving the text file: {}", e),
    }
}

fn create_pdf_single(text_file_path: &str, images: Vec<std::result::Result<Image, anyhow::Error>>) {
    let (doc, mut page, mut layer) =
        PdfDocument::new("PDF_Document_title", Mm(PAGE_X), Mm(PAGE_Y), "Layer 1");
    let images_length = images.len();
    for (i, image) in images.into_iter().enumerate() {
        match image {
            Ok(image) => {
                let current_layer_ref = doc.get_page(page).get_layer(layer);
                let x = Mm(PAGE_X / 2.0 - (CARD_WIDTH_MM / 2.0));
                let y = Mm(PAGE_Y / 2.0 - (CARD_HEIGHT_MM / 2.0));
                image.add_to_layer(
                    current_layer_ref,
                    ImageTransform {
                        translate_x: Some(x),
                        translate_y: Some(y),
                        ..Default::default()
                    },
                );

                if i < images_length - 1 {
                    let (new_page, new_layer) = doc.add_page(Mm(PAGE_X), Mm(PAGE_Y), "new_layer");
                    page = new_page;
                    layer = new_layer;
                }
            }
            Err(e) => eprintln!("Error getting image: {}", e),
        }
    }
    match save_pdf(text_file_path, doc) {
        Ok(pdf_filepath) => {
            println!("Saving pdf to path: {}", pdf_filepath);
            open_file_in_explorer(&pdf_filepath);
        }
        Err(e) => eprintln!("Error saving the text file: {}", e),
    }
}

fn open_file_in_explorer(file_path: &str) {
    let command = match OS {
        "linux" => "xdg-open",
        "macos" => "open",
        "windows" => "explorer",
        _ => "",
    };
    Command::new(command).arg(file_path).spawn().unwrap();
}

fn save_pdf(file_path: &str, doc: PdfDocumentReference) -> Result<String> {
    let stem: Vec<&str> = file_path.split(".").collect();
    let pdf_filepath = format!("{}{}", stem[0], ".pdf");
    let file = File::create(&pdf_filepath)?;
    let mut writer = BufWriter::new(file);
    doc.save(&mut writer)?;
    Ok(pdf_filepath)
}

async fn get_card_image_url(
    client: &Client,
    card_name: &str,
    set_name: Option<&str>,
    image_file_type: &str,
) -> Result<CardImageUrls> {
    println!(
        "Creating image URL for card '{}'{}",
        card_name,
        set_name
            .map(|s| format!(" from set '{}'", s))
            .unwrap_or_default()
    );

    // URL encoding for card_name and set_name
    let encoded_card_name = encode(card_name);
    let base_url = "https://api.scryfall.com/cards/named";

    let url = match set_name {
        Some(set) => {
            let encoded_set_name = encode(set);
            format!(
                "{}?fuzzy={}&set={}",
                base_url, encoded_card_name, encoded_set_name
            )
        }
        None => format!("{}?fuzzy={}", base_url, encoded_card_name),
    };

    println!("[Scryfall API] Requesting image URL from: '{}'", url);

    let res = client
        .get(&url)
        .send()
        .await
        .context("Failed to make request to Scryfall API")?;

    println!("Scryfall Request Response Satus: {}", res.status());

    if res.status().is_success() {
        let data: serde_json::Value = res.json().await.context("Failed to parse JSON response")?;

        if let Some(image_uris) = data["image_uris"].as_object() {
            if let Some(png_url) = image_uris.get(image_file_type) {
                return Ok(CardImageUrls {
                    front: Some(
                        png_url
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Image URL is not a valid string"))?
                            .to_string(),
                    ),
                    back: None,
                });
            }
        }

        if let Some(card_faces) = data["card_faces"].as_array() {
            let mut front_image_urls: Vec<String> = Vec::new();

            for card_face in card_faces {
                let image_uris = card_face["image_uris"]
                    .as_object()
                    .context("Field 'image_uris' not found in JSON response")?;
                if let Some(png_url) = image_uris.get(image_file_type) {
                    front_image_urls.push(
                        png_url
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Image URL is not a valid string"))?
                            .to_string(),
                    );
                }
            }

            // Check for card back images if present
            if front_image_urls.len() == 2 {
                let front = front_image_urls.get(0).cloned();
                let back = front_image_urls.get(1).cloned();
                return Ok(CardImageUrls { front, back });
            } else {
                let front = front_image_urls.get(0).cloned();
                return Ok(CardImageUrls { front, back: None });
            }
        }
        // If no image_uris or card_faces were found
        anyhow::bail!("Image URLs not found in JSON response");
    } else {
        anyhow::bail!(
            "Error: Failed to retrieve card data. Status Code: {}",
            res.status()
        );
    }
}

async fn get_card_image(png_url: Option<String>) -> Result<Image> {
    if let Some(url) = png_url {
        println!("[Download] Downloading image from URL: {}", url);
        // downloading image from url to bytes
        let response = reqwest::get(url)
            .await
            .context("Failed to fetch image from URL")?;
        let img_bytes = response
            .bytes()
            .await
            .context("Could not convert URL to bytes")?;

        // transforming image bytes to image format required by printpdf
        let mut reader = Cursor::new(img_bytes);

        let decoder = PngDecoder::new(&mut reader).context("Failed to create PNG decoder")?;

        let mut image = Image::try_from(decoder).context("Failed to create image from data")?;

        image.image = remove_alpha_channel_from_image_x_object(image.image);
        Ok(image)
    } else {
        println!("[Download] Using local card back image.");
        let img_reader = ImageReader::new(Cursor::new(CARDBACK_IMAGE))
            .with_guessed_format()
            .context("Failed to open card back image")?;
        let dynamic_image = img_reader
            .decode()
            .context("Failed to decode local image")?;
        let mut image = Image::from_dynamic_image(&dynamic_image);
        image.image = remove_alpha_channel_from_image_x_object(image.image);
        Ok(image)
    }
}

async fn parse_text_file(file: File) -> io::Result<Vec<(String, Option<String>)>> {
    let mut card_details = Vec::new();
    let card_pattern_with_set = Regex::new(r"\d (.*) \(").unwrap();
    let card_pattern_without_set = Regex::new(r"\d (.*)").unwrap();
    let set_pattern = Regex::new(r"\(([a-zA-Z0-9]*)\)").unwrap();

    for line in io::BufReader::new(file).lines() {
        let line = line?;

        // TODO: Check if this causes problems with certain decklist formats
        // Choose regex based on presence of '('
        let card_pattern = if line.contains('(') {
            &card_pattern_with_set
        } else {
            &card_pattern_without_set
        };

        if let Some(card_match) = card_pattern.captures(&line) {
            let card_name = card_match[1].trim().to_string();
            let set_name = set_pattern.captures(&line).map(|cap| cap[1].to_string());
            card_details.push((card_name, set_name))
        } else {
            // Handle lines that don't match the expected format
            eprintln!("Warning: Skipped line - {}", line.trim());
        }
    }

    Ok(card_details)
}

// taken from https://github.com/fschutt/printpdf/issues/119
pub fn remove_alpha_channel_from_image_x_object(image_x_object: ImageXObject) -> ImageXObject {
    if !matches!(image_x_object.color_space, ColorSpace::Rgba) {
        return image_x_object;
    };
    let ImageXObject {
        color_space,
        image_data,
        ..
    } = image_x_object;

    let new_image_data = image_data
        .chunks(4)
        .map(|rgba| {
            let [red, green, blue, alpha]: [u8; 4] = rgba.try_into().ok().unwrap();
            let alpha = alpha as f64 / 255.0;
            let new_red = ((1.0 - alpha) * 255.0 + alpha * red as f64) as u8;
            let new_green = ((1.0 - alpha) * 255.0 + alpha * green as f64) as u8;
            let new_blue = ((1.0 - alpha) * 255.0 + alpha * blue as f64) as u8;
            return [new_red, new_green, new_blue];
        })
        .collect::<Vec<[u8; 3]>>()
        .concat();

    let new_color_space = match color_space {
        ColorSpace::Rgba => ColorSpace::Rgb,
        ColorSpace::GreyscaleAlpha => ColorSpace::Greyscale,
        other_type => other_type,
    };

    ImageXObject {
        color_space: new_color_space,
        image_data: new_image_data,
        ..image_x_object
    }
}
