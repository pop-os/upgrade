use serde_derive::Deserialize;
use thiserror::Error;

const BASE: &str = "https://api.pop-os.org/";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("build ({}) is not a number", _0)]
    BuildNaN(String),

    #[error("failed to GET release API")]
    Get(#[from] isahc::Error),

    #[error("failed to parse JSON response")]
    Json(#[from] serde_json::Error),

    #[error("server returned an error status: {:?}", _0)]
    Status(isahc::http::StatusCode),
}

#[derive(Debug, Deserialize)]
pub struct RawRelease {
    pub version: String,
    pub url:     String,
    pub size:    u64,
    pub sha_sum: String,
    pub channel: String,
    pub build:   String,
    pub urgent:  String,
}

impl RawRelease {
    fn into_release(self) -> Result<Release, ApiError> {
        let RawRelease { version, url, size, sha_sum, channel, build, urgent } = self;
        let build = build.parse::<u16>().map_err(|_| ApiError::BuildNaN(build))?;
        let urgent = urgent == "true";

        Ok(Release { version, url, size, sha_sum, channel, build, urgent })
    }
}

#[derive(Debug)]
pub struct Release {
    pub version: String,
    pub url:     String,
    pub size:    u64,
    pub sha_sum: String,
    pub channel: String,
    pub build:   u16,
    pub urgent:  bool,
}

impl Release {
    pub fn get_release(version: &str, channel: &str) -> Result<Release, ApiError> {
        info!("checking for build {} in channel {}", version, channel);
        let url = [BASE, "builds/", version, "/", channel].concat();

        let response =
            crate::misc::http_client().map_err(ApiError::Get)?.get(&url).map_err(ApiError::Get)?;

        let status = response.status();
        if !status.is_success() {
            return Err(ApiError::Status(status));
        }

        serde_json::from_reader::<_, RawRelease>(response.into_body())
            .map_err(ApiError::Json)?
            .into_release()
    }

    pub fn build_exists(version: &str, channel: &str) -> Result<u16, ApiError> {
        Self::get_release(version, channel).map(|r| r.build)
    }
}

#[test]
pub fn release_exists() {
    let result = Release::get_release("20.04", "intel");
    assert!(result.is_ok());
}
