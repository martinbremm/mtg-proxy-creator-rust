extern crate image;
extern crate printpdf;

use std::fs::File;
use std::io::{self, BufRead, BufWriter, Cursor};

use anyhow::{Context, Result};
use futures::future::join_all;
use image::codecs::png::PngDecoder;
use image::io::Reader as ImageReader;
use printpdf::*;
use regex::Regex;
use reqwest;
use rfd::FileDialog;
use urlencoding::encode;

const CARDBACK_IMAGE: &[u8] = include_bytes!("../image/magic_card_back.png");

#[derive(Debug)]
struct CardImage {
    front: Option<String>,
    back: Option<String>,
}

impl IntoIterator for CardImage {
    type Item = Option<String>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        vec![self.front, self.back].into_iter()
    }
}

#[tokio::main]
async fn main() {
    let x = 210.0;
    let y = 297.0;

    let file = FileDialog::new()
        .set_directory("./input")
        .add_filter("text", &["txt"])
        .pick_file();

    let selected_file = match file {
        None => {
            eprintln!("Please select a .txt file including the decklist.");
            return;
        }
        Some(file) => file,
    };

    let start: std::time::Instant = std::time::Instant::now();

    // pdf creation
    let (doc, _, _) = PdfDocument::new("PDF_Document_title", Mm(x), Mm(y), "Layer 1");

    let text_file_path = match selected_file.into_os_string().into_string() {
        Ok(text_file_path) => text_file_path,
        Err(e) => {
            eprint!("There was an error parsing the file path: {:?}", e);
            return;
        }
    };

    let card_data = match parse_text_file(&text_file_path).await {
        Ok(card_data) => card_data,
        Err(e) => {
            eprintln!("Error parsing the text file: {}", e);
            return;
        }
    };

    let mut image_futures = vec![];
    for (card_name, set_name) in card_data {
        if let Ok(card_image) = get_card_image_url(&card_name, &set_name).await {
            for image_url in card_image {
                let image_future = tokio::spawn(get_card_image(image_url));
                image_futures.push(image_future);
            }
        } else {
            eprintln!(
                "Error retrieving png url for card: {} from set: {}",
                card_name, set_name
            );
        }
    }

    let images = join_all(image_futures).await;

    for image_result in images {
        match image_result {
            Ok(image) => {
                match image {
                    Ok(image) => {
                        let (new_page, new_layer) = doc.add_page(Mm(x), Mm(y), "new page");

                        let current_layer = doc.get_page(new_page).get_layer(new_layer);

                        image.add_to_layer(
                            current_layer.clone(),
                            ImageTransform {
                                // centering image on the page (mtg card size = 63*88 mm)
                                translate_x: Some(Mm(x / 2.0 - 63.0 / 2.0)),
                                translate_y: Some(Mm(y / 2.0 - 88.0 / 2.0)),
                                ..Default::default()
                            },
                        );
                    }
                    Err(e) => eprintln!("Error getting image: {}", e),
                }
            }
            Err(e) => eprintln!("Error resolving image future: {}", e),
        }
    }

    match save_pdf(&text_file_path, doc) {
        Ok(saved) => saved,
        Err(e) => eprintln!("Error saving the text file: {}", e),
    }

    eprintln!("{:.2?}", start.elapsed());
}

fn save_pdf(file_path: &str, doc: PdfDocumentReference) -> Result<(), String> {
    let stem: Vec<&str> = file_path.split(".").collect();
    let pdf_filename = format!("{}{}", stem[0], ".pdf");
    doc.save(&mut BufWriter::new(
        File::create(pdf_filename).map_err(|e| format!("Error creating file: {}", e))?,
    ))
    .map_err(|e| format!("Error saving PDF: {}", e))
}

async fn get_card_image_url(card_name: &str, set_name: &str) -> Result<CardImage> {
    // URL encoding for card_name and set_name
    let card_name = encode(card_name);
    let set_name = encode(set_name);

    let url = format!(
        "https://api.scryfall.com/cards/named?fuzzy={}&set={}",
        card_name, set_name
    );

    let res = reqwest::get(&url)
        .await
        .context("Failed to make request to Scryfall API")?;

    if res.status().is_success() {
        let data: serde_json::Value = res.json().await.context("Failed to parse JSON response")?;

        if let Some(image_uris) = data["image_uris"].as_object() {
            if let Some(png_url) = image_uris.get("png") {
                return Ok(CardImage {
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
                if let Some(png_url) = image_uris.get("png") {
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
                return Ok(CardImage { front, back });
            } else {
                let front = front_image_urls.get(0).cloned();
                return Ok(CardImage { front, back: None });
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

async fn parse_text_file(txt_path: &str) -> io::Result<Vec<(String, String)>> {
    let mut card_names = Vec::new();
    let mut set_names = Vec::new();
    let card_pattern = Regex::new(r"\d (.*) \(").unwrap();
    let set_pattern = Regex::new(r"\(([a-zA-Z0-9]*)\)").unwrap();

    let file = File::open(txt_path)?;

    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if let (Some(card_match), Some(set_match)) =
            (card_pattern.captures(&line), set_pattern.captures(&line))
        {
            card_names.push(card_match[1].to_string());
            set_names.push(set_match[1].to_string());
        } else {
            // Handle lines that don't match the expected format
            eprintln!("Warning: Skipped line - {}", line.trim());
        }
    }

    Ok(card_names.into_iter().zip(set_names).collect())
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
