use std::borrow::Cow;
use std::path::{Path, PathBuf};

#[cfg(feature = "embedded-web")]
use rust_embed::RustEmbed;

#[cfg(feature = "embedded-web")]
#[derive(RustEmbed)]
#[folder = "target/web/"]
struct EmbeddedWebAssets;

pub struct WebAsset {
    pub bytes: Cow<'static, [u8]>,
    pub content_type: &'static str,
    pub cache_control: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAssetError {
    Forbidden,
    NotFound,
    #[cfg_attr(feature = "embedded-web", allow(dead_code))]
    Internal,
}

pub fn load_web_asset(_web_dir: &str, relative_path: &str) -> Result<WebAsset, WebAssetError> {
    let normalized_path = normalize_relative_path(relative_path)?;

    #[cfg(feature = "embedded-web")]
    {
        if let Some(asset) = EmbeddedWebAssets::get(normalized_path.as_str()) {
            return Ok(WebAsset {
                bytes: asset.data,
                content_type: content_type_for_path(&normalized_path),
                cache_control: cache_control_for_path(&normalized_path),
            });
        }
        return Err(WebAssetError::NotFound);
    }

    #[cfg(not(feature = "embedded-web"))]
    {
        load_web_asset_from_fs(_web_dir, &normalized_path)
    }
}

fn normalize_relative_path(relative_path: &str) -> Result<String, WebAssetError> {
    if relative_path.is_empty() {
        return Err(WebAssetError::NotFound);
    }

    if relative_path.starts_with('/') {
        return Err(WebAssetError::Forbidden);
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(relative_path).components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return Err(WebAssetError::Forbidden),
        }
    }

    let normalized = normalized.to_string_lossy().replace('\\', "/");
    if normalized.is_empty() {
        return Err(WebAssetError::NotFound);
    }

    Ok(normalized)
}

#[cfg(not(feature = "embedded-web"))]
fn load_web_asset_from_fs(web_dir: &str, relative_path: &str) -> Result<WebAsset, WebAssetError> {
    let root = std::fs::canonicalize(web_dir).map_err(|_| WebAssetError::Internal)?;
    let full_path = root.join(relative_path);
    let canonical_path = std::fs::canonicalize(&full_path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            WebAssetError::NotFound
        } else {
            WebAssetError::Internal
        }
    })?;

    if !canonical_path.starts_with(&root) {
        return Err(WebAssetError::Forbidden);
    }

    let bytes = std::fs::read(&canonical_path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            WebAssetError::NotFound
        } else {
            WebAssetError::Internal
        }
    })?;

    Ok(WebAsset {
        bytes: Cow::Owned(bytes),
        content_type: content_type_for_path(relative_path),
        cache_control: cache_control_for_path(relative_path),
    })
}

fn content_type_for_path(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
}

fn cache_control_for_path(path: &str) -> Option<&'static str> {
    if path.starts_with("_next/static/") {
        Some("public, max-age=31536000, immutable")
    } else {
        None
    }
}
