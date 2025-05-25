use std::path::PathBuf;

use iced::widget::{button, center, column, radio, slider, text};
use iced::{Center, Element, Fill};
use rfd::FileDialog;

mod proxy;

pub fn main() -> iced::Result {
    iced::run("Proxy Creator", ProxyConfig::update, ProxyConfig::view)
}

#[derive(Default)]
struct ProxyConfig {
    selected_schema: bool,
    padding_value: f32,
    file_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
enum Message {
    SchemaChange(bool),
    PaddingChanged(f32),
    FileSelectButtonPressed,
}

impl ProxyConfig {
    fn update(&mut self, message: Message) {
        match message {
            Message::SchemaChange(schema) => {
                self.selected_schema = schema;
            }
            Message::PaddingChanged(padding) => {
                self.padding_value = padding;
            }
            Message::FileSelectButtonPressed => {
                // Block until user selects file
                let selected_file_path = FileDialog::new()
                    .set_directory("./input")
                    .add_filter("Text Files", &["txt"])
                    .pick_file();

                proxy::run(selected_file_path.clone(), self.selected_schema);

                self.file_path = selected_file_path;
            }
        }
    }

    fn view(&self) -> Element<Message> {
        let file_button = column![
            button("Select .txt file").on_press(Message::FileSelectButtonPressed),
            text(
                self.file_path
                    .as_ref()
                    .map(|p| format!("Selected file: {}", p.display()))
                    .unwrap_or("No file selected".to_string())
            )
        ]
        .width(Fill)
        .align_x(Center);

        let one_by_one = radio(
            "One card per page",
            false,
            Some(self.selected_schema),
            Message::SchemaChange,
        );

        let matrix = radio(
            "3x3 card matrix",
            true,
            Some(self.selected_schema),
            Message::SchemaChange,
        );

        let choose_schema = column![text("Schema:"), one_by_one, matrix]
            .spacing(10)
            .width(Fill)
            .align_x(Center);

        let padding_slider = if self.selected_schema {
            column![
                text("Padding"),
                slider(0.0..=100.0, self.padding_value, Message::PaddingChanged),
                text(format!("{} mm", self.padding_value))
            ]
            .width(Fill)
            .align_x(Center)
        } else {
            column![]
        };

        let content = column![file_button, choose_schema, padding_slider]
            .spacing(20)
            .padding(20)
            .max_width(600);

        center(content).into()
    }
}

#[test]
fn change_config_properly() {
    let mut config = ProxyConfig {
        selected_schema: false,
        padding_value: 50.0,
        file_path: None,
    };

    config.update(Message::SchemaChange(true));
    config.update(Message::PaddingChanged(70.0));

    assert_eq!(config.selected_schema, true);
    assert_eq!(config.padding_value, 70.0)
}
