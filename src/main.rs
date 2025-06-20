use std::path::PathBuf;
use std::process::Command;

use iced::widget::{button, center, column, radio, slider, text};
use iced::{Center, Element, Fill, Task};
use rfd::FileDialog;

mod proxy;

pub fn main() -> iced::Result {
    iced::run("Proxy Creator", ProxyConfig::update, ProxyConfig::view)
}

#[derive(Default)]
struct ProxyConfig {
    selected_schema: bool,
    padding_value: f64,
    file_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
enum Message {
    SchemaChange(bool),
    PaddingChanged(f64),
    FileSelectButtonPressed,
    StartButtonPressed,
    ProxyPdfFileCreated(Option<PathBuf>),
}

fn open_file(path: PathBuf) {
    #[cfg(target_os = "windows")]
    let mut cmd = Command::new("explorer");

    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");

    #[cfg(target_os = "linux")]
    let mut cmd = Command::new("xdg-open");

    cmd.arg(path);

    // This will block, but if you don't mind, it's fine for quick tasks
    cmd.spawn().unwrap();
}

impl ProxyConfig {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SchemaChange(schema) => {
                self.selected_schema = schema;
                Task::none()
            }
            Message::PaddingChanged(padding) => {
                self.padding_value = padding;
                Task::none()
            }
            Message::FileSelectButtonPressed => {
                // Block until user selects file
                let selected_file_path = FileDialog::new()
                    .set_directory("./input")
                    .add_filter("Text Files", &["txt"])
                    .pick_file();

                self.file_path = selected_file_path;
                Task::none()
            }
            Message::StartButtonPressed => Task::perform(
                proxy::run(
                    self.file_path.clone(),
                    self.selected_schema,
                    self.padding_value,
                ),
                Message::ProxyPdfFileCreated,
            ),
            Message::ProxyPdfFileCreated(pdf_path_opt) => {
                if let Some(pdf_path_opt) = pdf_path_opt {
                    open_file(pdf_path_opt);
                } else {
                    eprintln!("PDF creation failed, no file path.");
                }
                Task::none()
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
                slider(0.0..=10.5, self.padding_value, Message::PaddingChanged),
                text(format!("{} mm", self.padding_value))
            ]
            .width(Fill)
            .align_x(Center)
        } else {
            column![]
        };

        let mut start_button = button("Create Proxies");

        if self.file_path.is_some() {
            start_button = start_button.on_press(Message::StartButtonPressed);
        }

        let start_button = column![start_button].width(Fill).align_x(Center);

        let content = column![file_button, choose_schema, padding_slider, start_button]
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

    let _ = config.update(Message::SchemaChange(true));
    let _ = config.update(Message::PaddingChanged(70.0));

    assert_eq!(config.selected_schema, true);
    assert_eq!(config.padding_value, 70.0)
}
