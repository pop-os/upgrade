use err_derive::Error;
use reqwest::{self, Client};
use serde_derive::Deserialize;

const BASE: &str = "https://api.pop-os.org/";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error(display = "build ({}) is not a number", _0)]
    BuildNaN(String),
    #[error(display = "failed to GET release API: {}", _0)]
    Get(reqwest::Error),
    #[error(display = "failed to parse JSON response: {}", _0)]
    Json(serde_json::Error),
}

#[derive(Debug, Deserialize)]
pub struct RawRelease {
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha_sum: String,
    pub channel: String,
    pub build: String,
}

impl RawRelease {
    fn into_release(self) -> Result<Release, ApiError> {
        let RawRelease { version, url, size, sha_sum, channel, build } = self;
        let build = build.parse::<u16>().map_err(|_| ApiError::BuildNaN(build))?;

        Ok(Release { version, url, size, sha_sum, channel, build })
    }
}

#[derive(Debug)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha_sum: String,
    pub channel: String,
    pub build: u16,
}

impl Release {
    pub fn get_release(version: &str, channel: &str) -> Result<Release, ApiError> {
        info!("checking for build {} in channel {}", version, channel);
        let url = [BASE, "builds/", version, "/", channel].concat();

        let response = Client::new()
            .get(&url)
            .send()
            .map_err(ApiError::Get)?
            .error_for_status()
            .map_err(ApiError::Get)?;

        serde_json::from_reader::<_, RawRelease>(response).map_err(ApiError::Json)?.into_release()
    }

    pub fn exists(current: &str, iso: &str) -> Option<u16> {
        Self::get_release(current, iso).ok().map(|r| r.build)
    }
}

#[test]
pub fn release_exists() {
    let result = Release::get_release("18.10", "intel");
    eprintln!("{:?}", result);
    assert!(result.is_ok());
}
