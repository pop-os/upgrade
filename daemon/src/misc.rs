use anyhow::Context;
use std::{fs, future::Future, io, path::Path, time::Duration};
use tokio::fs::{copy, File};

pub fn http_client() -> Result<isahc::HttpClient, isahc::Error> {
    use isahc::config::Configurable;

    isahc::HttpClient::builder()
        .low_speed_timeout(1, std::time::Duration::from_secs(1))
        .redirect_policy(isahc::config::RedirectPolicy::Follow)
        .build()
}

// Default options used by all apt-get invocations in pop-upgrade.
pub fn apt_get() -> apt_cmd::AptGet {
    apt_cmd::AptGet::new()
        .noninteractive()
        .force()
        .force_confdef()
        .force_confold()
        .allow_downgrades()
}

pub async fn create<P: AsRef<Path>>(path: P) -> io::Result<File> {
    File::create(&path).await.map_err(|why| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("unable to create file at {:?}: {}", path.as_ref(), why),
        )
    })
}

pub async fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
    File::open(&path).await.map_err(|why| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("unable to open file at {:?}: {}", path.as_ref(), why),
        )
    })
}

pub async fn cp<'a>(src: &'a Path, dst: &'a Path) -> io::Result<u64> {
    copy(src, dst).await.map_err(|why| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("failed to copy {:?} to {:?}: {}", src, dst, why),
        )
    })
}

pub fn format_build_number(value: i16, buffer: &mut String) -> &str {
    if value < 0 {
        "false"
    } else {
        *buffer = format!("{}", value);
        buffer.as_str()
    }
}

pub fn format_error(source: &(dyn std::error::Error + 'static)) -> String {
    let mut out = fomat!((source));

    let mut source = source.source();
    while let Some(why) = source {
        out.push_str(&fomat!(": "(why)));
        source = why.source();
    }

    out
}

pub fn uid_min_max() -> anyhow::Result<(u32, u32)> {
    let login_defs =
        fs::read_to_string("/etc/login.defs").context("could not read /etc/login.defs")?;

    let defs = whitespace_conf::parse(&login_defs);

    defs.get("UID_MIN")
        .zip(defs.get("UID_MAX"))
        .context("/etc/login.defs does not contain UID_MIN + UID_MAX")
        .and_then(|(min, max)| {
            let min = min.parse::<u32>().context("UID_MIN is not a u32 value")?;
            let max = max.parse::<u32>().context("UID_MAX is not a u32 value")?;
            Ok((min, max))
        })
}

pub async fn network_reconnect<Fun, Fut, Res>(func: Fun) -> Res
where
    Fun: Fn() -> Fut,
    Fut: Future<Output = Res>,
{
    loop {
        let request = func();
        let changed = async_fetcher::iface::watch_change();

        futures::pin_mut!(request);
        futures::pin_mut!(changed);

        use futures::future::Either;

        match futures::future::select(request, changed).await {
            Either::Left((result, _)) => break result,
            Either::Right(_) => tokio::time::sleep(Duration::from_secs(3)).await,
        }
    }
}
