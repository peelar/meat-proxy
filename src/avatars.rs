use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::model::PullRequest;
use rune::{Image, ImageCache, Latest};

#[derive(Default)]
pub struct Avatars {
    images: HashMap<String, Image>,
    retry_after: HashMap<String, Instant>,
    worker: Latest<(String, Option<Image>)>,
    decoder: Option<Arc<ImageCache>>,
}

impl Avatars {
    pub fn setup(&mut self, decoder: Arc<ImageCache>, waker: rune::Waker) {
        self.decoder = Some(decoder);
        self.worker.set_waker(waker);
    }

    pub fn get(&self, login: &str) -> Option<&Image> {
        self.images.get(login)
    }

    pub fn load_preview(&mut self, prs: &[PullRequest]) {
        let Some(decoder) = &self.decoder else { return };
        for pr in prs {
            if !self.images.contains_key(&pr.author)
                && let Some(image) = load(&pr.author, decoder, false)
            {
                self.images.insert(pr.author.clone(), image);
            }
        }
    }

    pub fn poll(&mut self, prs: &[PullRequest]) -> bool {
        let mut changed = false;
        if let Some((login, image)) = self.worker.take() {
            if let Some(image) = image {
                self.images.insert(login, image);
                changed = true;
            } else {
                self.retry_after
                    .insert(login, Instant::now() + Duration::from_secs(900));
            }
        }
        if !self.worker.is_pending()
            && let Some(decoder) = self.decoder.clone()
            && let Some(pr) = prs.iter().find(|pr| {
                !self.images.contains_key(&pr.author)
                    && self
                        .retry_after
                        .get(&pr.author)
                        .is_none_or(|when| *when <= Instant::now())
            })
        {
            self.worker.request(pr.author.clone(), move |login| {
                let image = load(&login, &decoder, true);
                (login, image)
            });
        }
        changed
    }
}

fn cache_path(login: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
        })?;
    Some(
        dir.join("meat-proxy/avatars")
            .join(login.to_ascii_lowercase()),
    )
}

fn decode(bytes: Vec<u8>, decoder: &ImageCache) -> Option<Image> {
    let image = decoder.load(bytes);
    image.wait().ok()?;
    Some(image)
}

fn load(login: &str, decoder: &ImageCache, network: bool) -> Option<Image> {
    let account = login.strip_suffix("[bot]").unwrap_or(login);
    if account.is_empty()
        || login.len() > 128
        || !account
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return None;
    }
    let path = cache_path(login);
    let cached = path
        .as_ref()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| decode(bytes, decoder));
    let fresh = path
        .as_ref()
        .and_then(|p| fs::metadata(p).ok()?.modified().ok()?.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(7 * 86400));
    if !network || (cached.is_some() && fresh) {
        return cached;
    }

    let encoded = login.replace("[bot]", "%5Bbot%5D");
    let response = Command::new("/usr/bin/curl")
        .args([
            "-q",
            "--fail",
            "--silent",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "5",
            "--max-filesize",
            "262144",
        ])
        .arg(format!(
            "https://avatars.githubusercontent.com/{encoded}?s=48"
        ))
        .output()
        .ok()
        .filter(|output| output.status.success());
    let Some(bytes) = response.map(|output| output.stdout) else {
        return cached;
    };
    let Some(image) = decode(bytes.clone(), decoder) else {
        return cached;
    };
    if let Some(path) = path
        && let Some(dir) = path.parent()
        && fs::create_dir_all(dir).is_ok()
    {
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        if fs::write(&temp, bytes).is_ok() {
            let _ = fs::rename(&temp, path);
        }
    }
    Some(image)
}
