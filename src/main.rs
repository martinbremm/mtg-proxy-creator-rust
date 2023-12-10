extern crate printpdf;
extern crate image;

use std::fs::File;
use std::io::{self, BufRead, BufWriter, Cursor};

use anyhow::{Result, Context};
use image::codecs::png::PngDecoder;
use printpdf::*;
use regex::Regex;
use reqwest;
use rfd::FileDialog;
use urlencoding::encode;


#[tokio::main]
async fn main() {

    let x = 210.0;
    let y = 297.0;

    // pdf creation
    let (doc, _, _) = PdfDocument::new("PDF_Document_title", Mm(x), Mm(y), "Layer 1");

    let file = FileDialog::new()
        .set_directory("./input")
        .add_filter("text", &["txt"])
        .pick_file();

    match file {
        None => eprintln!("Please select a .txt file including the decklist."),
        Some(file) => {
            let text_file_path = file.into_os_string().into_string().unwrap();

            match parse_text_file(&text_file_path).await {

                Ok(card_data) => {
                    
                    for (card_name, set_name) in card_data {
                    
                        match get_card_image_url(&card_name, &set_name).await {

                            Ok(image_url) => match get_card_image(&image_url).await {

                                Ok(image) => {

                                    let (new_page, new_layer) = doc.add_page(Mm(x), Mm(y), "new page");

                                    let current_layer = doc.get_page(new_page).get_layer(new_layer);

                                    image.add_to_layer(
                                        current_layer.clone(), 
                                        ImageTransform {
                                            // centering image on the page (mtg card size = 63*88 mm)
                                            translate_x: Some(Mm(x/2.0 - 63.0/2.0)),
                                            translate_y: Some(Mm(y/2.0 - 88.0/2.0)),
                                            ..Default::default()
                                        },
                                    );
                                },

                                Err(e) => eprintln!("Error adding image to current page: {}", e),
                            }

                            Err(e) => eprintln!("Error retrieving png url: {}", e),
                        }
                    }
                }
                Err(e) => eprintln!("Error parsing the text file: {}", e),
            }
            
            match save_pdf(&text_file_path, doc) {
                Ok(saved) => saved,
                Err(e) => eprintln!("Error saving the text file: {}", e),
            }

        }
    }
}


fn save_pdf(file_path: &str, doc: PdfDocumentReference) -> Result<(), String> {
    let stem: Vec<&str> = file_path.split(".").collect();
    let pdf_filename = format!("{}{}", stem[0], ".pdf");
    doc.save(&mut BufWriter::new(File::create(pdf_filename).map_err(|e| format!("Error creating file: {}", e))?))
        .map_err(|e| format!("Error saving PDF: {}", e))
}


async fn get_card_image_url(card_name: &str, set_name: &str) -> Result<String> {
    // URL encoding for card_name and set_name
    let card_name = encode(card_name);
    let set_name = encode(set_name);

    let url = format!(
        "https://api.scryfall.com/cards/named?fuzzy={}&set={}",
        card_name, set_name
    );

    let res = reqwest::get(&url).await.context("Failed to make request to Scryfall API")?;

    if res.status().is_success() {
        let data: serde_json::Value = res.json().await.context("Failed to parse JSON response")?;
        let image_uris = data["image_uris"].as_object().context("Field 'image_uris' not found in JSON response")?;
        let png_url = image_uris.get("png").context("Image URL not found in JSON response")?;
        Ok(png_url.as_str().ok_or_else(|| anyhow::anyhow!("Image URL is not a valid string"))?.to_string())
    } else {
        anyhow::bail!("Error: Failed to retrieve card data. Status Code: {}", res.status());
    }
}


async fn get_card_image(png_url: &str) -> Result<Image> {
    // downloading image from url to bytes
    let img_bytes = reqwest::get(png_url).await?.bytes().await.context("Could not convert URL to bytes")?;

    // transforming image bytes to image format required by printpdf
    let mut reader = Cursor::new(img_bytes.as_ref());

    let decoder = PngDecoder::new(&mut reader).unwrap();
    let mut image = Image::try_from(decoder).unwrap();

    image.image = remove_alpha_channel_from_image_x_object(image.image);

    Ok(image)
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


// taken from https://github.com/fschutt/printpdf/issues/119
pub fn remove_alpha_channel_from_image_x_object(image_x_object: ImageXObject) -> ImageXObject {
    if !matches!(image_x_object.color_space, ColorSpace::Rgba)
    {
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