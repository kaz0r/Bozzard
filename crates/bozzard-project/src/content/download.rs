use super::*;
use std::{
    fs,
    io::Read,
    sync::OnceLock,
    time::{Duration, Instant},
};
use url::Url;

#[derive(Clone)]
enum Location {
    File(PathBuf),
    Http(Url),
}
impl Location {
    fn parse(value: &str) -> Result<Self> {
        if value.starts_with("http://") || value.starts_with("https://") {
            let url = Url::parse(value)?;
            validate_url(&url)?;
            Ok(Self::Http(url))
        } else {
            ensure!(
                !value.contains("://"),
                "content locations support files and HTTPS URLs"
            );
            Ok(Self::File(std::path::absolute(value)?))
        }
    }
    fn resolve(&self, value: &str) -> Result<Self> {
        if value.starts_with("http://") || value.starts_with("https://") {
            return Self::parse(value);
        }
        match self {
            Self::File(path) => {
                portable_path(value)?;
                Ok(Self::File(
                    path.parent().context("catalog parent")?.join(value),
                ))
            }
            Self::Http(base) => {
                let url = base.join(value)?;
                validate_url(&url)?;
                Ok(Self::Http(url))
            }
        }
    }
    fn open(&self, limit: u64, progress: &Progress) -> Result<(Box<dyn Read + Send>, Self)> {
        progress.check()?;
        match self {
            Self::File(path) => {
                let file = fs::File::open(path)
                    .with_context(|| format!("opening content {}", path.display()))?;
                ensure!(
                    file.metadata()?.len() <= limit,
                    "content source exceeds size limit"
                );
                Ok((Box::new(file), self.clone()))
            }
            Self::Http(url) => {
                progress.stage("Connecting to content server")?;
                let response = client()?
                    .get(url.clone())
                    .header("Accept-Encoding", "identity")
                    .send()?
                    .error_for_status()?;
                ensure!(
                    response.status().as_u16() == 200,
                    "content server must return a complete HTTP 200 response"
                );
                ensure!(
                    response.content_length().is_none_or(|n| n <= limit),
                    "download exceeds declared content size"
                );
                let actual = response.url().clone();
                validate_url(&actual)?;
                Ok((
                    Box::new(NetworkReader {
                        response,
                        started: Instant::now(),
                        progress: progress.clone(),
                    }),
                    Self::Http(actual),
                ))
            }
        }
    }
}
fn validate_url(url: &Url) -> Result<()> {
    let loopback = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && loopback),
        "content downloads require HTTPS; HTTP is supported on loopback for local development"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "content URLs cannot contain credentials or fragments"
    );
    Ok(())
}
fn client() -> Result<&'static reqwest::blocking::Client> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                // Blocking reqwest applies this timeout to individual body reads, so a large
                // progressing download is not limited to five seconds. Cancellation is checked
                // before each read; a stalled read ends within this same bound.
                .timeout(Duration::from_secs(5))
                .connect_timeout(Duration::from_secs(5))
                .pool_max_idle_per_host(2)
                .pool_idle_timeout(Duration::from_secs(30))
                .referer(false)
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 5 {
                        attempt.error("content redirect limit")
                    } else if validate_url(attempt.url()).is_err()
                        || (attempt.previous().iter().any(|u| u.scheme() == "https")
                            && attempt.url().scheme() != "https")
                    {
                        attempt.error("invalid content redirect")
                    } else {
                        attempt.follow()
                    }
                }))
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!(e.clone()))
}
struct NetworkReader {
    response: reqwest::blocking::Response,
    started: Instant,
    progress: Progress,
}
impl Read for NetworkReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.progress.check().map_err(std::io::Error::other)?;
        if self.started.elapsed() > Duration::from_secs(600) {
            return Err(std::io::Error::other(
                "content download exceeded ten minutes",
            ));
        }
        self.response.read(bytes)
    }
}

#[derive(Clone)]
pub struct ContentCatalog {
    pub catalog: Catalog,
    origin: Location,
}
pub fn load_catalog(location: &str, progress: &Progress) -> Result<ContentCatalog> {
    progress.stage("Loading content catalog")?;
    let (reader, origin) = Location::parse(location)?.open(1024 * 1024, progress)?;
    let mut bytes = Vec::new();
    reader.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "content catalog exceeds 1 MiB");
    let catalog: Catalog = serde_json::from_slice(&bytes)?;
    catalog.validate()?;
    progress.check()?;
    Ok(ContentCatalog { catalog, origin })
}

/// Installed generations are immutable and content-addressed. Old handles remain usable
/// across catalog updates. A store verifies a generation once, then shares its mounted Arc.
pub struct ContentStore {
    root: PathBuf,
    mounted: BTreeMap<String, Arc<MountedPack>>,
}
impl ContentStore {
    pub fn new(cache: impl Into<PathBuf>) -> Self {
        Self {
            root: cache.into(),
            mounted: BTreeMap::new(),
        }
    }
    pub fn resolve(
        &mut self,
        catalog: &ContentCatalog,
        name: &str,
        progress: &Progress,
    ) -> Result<ResolvedContent> {
        catalog.catalog.validate()?;
        let address = catalog
            .catalog
            .addresses
            .get(name)
            .with_context(|| format!("unknown content address '{name}'"))?;
        let reference = &catalog.catalog.packs[&address.pack];
        let pack = self.mount(catalog, &address.pack, reference, progress)?;
        let entry = pack
            .index
            .entries
            .get(&address.entry)
            .with_context(|| format!("pack has no entry '{}'", address.entry))?
            .clone();
        Ok(ResolvedContent { pack, entry })
    }
    fn mount(
        &mut self,
        catalog: &ContentCatalog,
        id: &str,
        reference: &PackReference,
        progress: &Progress,
    ) -> Result<Arc<MountedPack>> {
        progress.check()?;
        if let Some(pack) = self.mounted.get(&reference.sha256) {
            ensure!(
                pack.id() == id
                    && pack.reference.bytes == reference.bytes
                    && pack.reference.index_sha256 == reference.index_sha256,
                "catalog disagrees with mounted pack"
            );
            return Ok(pack.clone());
        }
        ensure!(self.mounted.len() < 1024, "mounted pack limit: 1024");
        // OS locks are released on process exit. No stale lock-file ownership or
        // simultaneous repair can race another installer of the same generation.
        fs::create_dir_all(&self.root)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(format!(".{}.lock", reference.sha256)))?;
        loop {
            progress.check()?;
            match lock.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) => {
                    progress.stage("Waiting for another content installer")?;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(fs::TryLockError::Error(e)) => {
                    return Err(e).context("locking content installation");
                }
            }
        }
        let root = self.root.join(&reference.sha256);
        let (index, typed_validation_needed) =
            match archive::verify_cache(&root, reference, progress) {
                Ok(index) => (index, true),
                Err(_) => {
                    progress.check()?;
                    let stage = archive::Staging::new(&self.root)?;
                    let candidate = stage.path.join("content");
                    fs::create_dir(&candidate)?;
                    let (reader, _) = catalog
                        .origin
                        .resolve(&reference.location)?
                        .open(reference.bytes, progress)?;
                    let index = archive::unpack(
                        reader,
                        &candidate,
                        reference,
                        &progress.subtask(0., 0.8)?,
                    )?;
                    ensure!(index.id == id, "downloaded pack id disagrees with catalog");
                    archive::validate_content(&candidate, &index, progress)?;
                    progress.check()?;
                    {
                        let backup = stage.path.join("previous");
                        let replaced = match fs::symlink_metadata(&root) {
                            Ok(_) => {
                                fs::rename(&root, &backup)?;
                                true
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                            Err(e) => return Err(e.into()),
                        };
                        if let Err(error) = fs::rename(&candidate, &root) {
                            if replaced {
                                let _ = fs::rename(&backup, &root);
                            }
                            return Err(error).context("publishing installed content");
                        }
                        (index, false)
                    }
                }
            };
        ensure!(index.id == id, "installed pack id disagrees with catalog");
        // Cached generations also receive typed validation before they are exposed.
        if typed_validation_needed {
            archive::validate_content(&root, &index, progress)?;
        }
        let pack = Arc::new(MountedPack {
            root,
            index,
            reference: reference.clone(),
        });
        self.mounted.insert(reference.sha256.clone(), pack.clone());
        progress.report(1, 1, "Content ready")?;
        Ok(pack)
    }
}

pub fn default_cache_directory() -> Result<PathBuf> {
    let directory = if cfg!(windows) {
        PathBuf::from(
            std::env::var_os("LOCALAPPDATA")
                .context("LOCALAPPDATA missing; choose a content cache directory")?,
        )
        .join("Bozzard/Content")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(
            std::env::var_os("HOME").context("HOME missing; choose a content cache directory")?,
        )
        .join("Library/Caches/dev.bozzard.engine/content")
    } else if let Some(root) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(root).join("bozzard/content")
    } else {
        PathBuf::from(
            std::env::var_os("HOME").context("HOME missing; choose a content cache directory")?,
        )
        .join(".cache/bozzard/content")
    };
    Ok(directory)
}
