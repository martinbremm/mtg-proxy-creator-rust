use mtg_proxy_creator_rust::proxy::main;
use std::fs;
use std::{path::PathBuf, str::FromStr};

const TEST_TXT_FILE_PATH: &str = "input/test.txt";

#[tokio::test]
async fn test_run_function() {
    let path = main(PathBuf::from_str(TEST_TXT_FILE_PATH).ok(), false, 0.0)
        .await
        .expect("main() failed");

    assert!(path.exists(), "PDF file does not exist");

    let metadata = fs::metadata(path).expect("Failed to get metadata");
    let size = metadata.len();
    assert!(size > 0, "PDF file is empty");
}
