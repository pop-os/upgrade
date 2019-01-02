use reqwest::{self, Client};
use serde_derive::Deserialize;
use err_derive::Error;

const BASE: &str = "https://api.pop-os.org/";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error(display = "failed to GET release API: {}", _0)]
    Get(reqwest::Error),
    #[error(display = "failed to parse JSON response: {}", _0)]
    Json(serde_json::Error)
}

#[derive(Debug, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha_sum: String,
    pub channel: String,
    pub build: String
}

impl Release {
    pub fn get_release(version: &str, arch: &str) -> Result<Release, ApiError> {
        let url = [BASE, "builds/", version, "/", arch].concat();

        let response = Client::new()
            .get(&url)
            .send()
            .map_err(ApiError::Get)?
            .error_for_status()
            .map_err(ApiError::Get)?;

        serde_json::from_reader(response).map_err(ApiError::Json)
    }
}
