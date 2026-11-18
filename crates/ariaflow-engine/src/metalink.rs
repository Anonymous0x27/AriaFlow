//! Metalink transport (mirror list + content checksums).
//!
//! Task URL form: `metalink:<https-url-of-the-.metalink-document>`, e.g.
//! `metalink:https://example.com/release.iso.metalink`
//!
//! The provider downloads the (small) Metalink document, picks the mirror with
//! the best `preference` attribute, and downloads the payload through the HTTP
//! provider — Metalink is a *manifest*, so checksums are passed through to the
//! orchestrator as `x-checksum-*` response headers.
//!
//! Supported: Metalink 4 (`<metalink xmlns="urn:ietf:params:xml:ns:metalink">`)
//! and Metalink 3 (`<metalinks><files><file ...>`) documents with a single
//! `<file>` section (the first one wins for multi-file documents). Parsing is
//! deliberately lenient and hand-rolled (no XML dependency).
//!
//! Limitations: `.torrent` embedded as a metalink resource is detected but
//! must be added as a `magnet:` link instead.

use crate::http::HttpProvider;
use ariaflow_protocol::{
    DownloadProvider, ProviderError, ProviderOptions, RemoteFileInfo, TransferOutcome,
    TransferProgress,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct MetalinkProvider {
    http: HttpProvider,
}

impl Default for MetalinkProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MetalinkProvider {
    pub fn new() -> Self {
        Self {
            http: HttpProvider::new(),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct MetalinkFile {
    name: Option<String>,
    size: Option<u64>,
    hashes: Vec<(String, String)>, // (type, hex digest)
    mirrors: Vec<(u32, String)>,   // (preference, url)
}

fn document_url(raw: &str) -> Result<String, ProviderError> {
    let url = url::Url::parse(raw)
        .map_err(|e| ProviderError::Unsupported(format!("invalid metalink URL: {e}")))?;
    if url.scheme() != "metalink" {
        return Err(ProviderError::Unsupported(format!(
            "Metalink provider cannot handle scheme {:?} (expected metalink:<https-url-of-.metalink-document>)",
            url.scheme()
        )));
    }
    // url::Url parks everything after `metalink:` into the path for non-special schemes.
    let mut doc = url.path().to_string();
    if doc.starts_with("//") {
        doc = doc.trim_start_matches('/').to_string();
        if let Some(rest) = doc.strip_prefix("localhost") {
            doc = format!("http://localhost{rest}");
        } else {
            doc = format!("https://{doc}");
        }
    }
    if !(doc.starts_with("http://") || doc.starts_with("https://")) {
        return Err(ProviderError::Unsupported(format!(
            "metalink URL must embed an http(s) document URL, e.g. metalink:https://example.com/file.iso.metalink (got {raw:?})"
        )));
    }
    Ok(doc)
}

async fn fetch_document(doc_url: &str, options: &ProviderOptions) -> Result<String, ProviderError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(std::time::Duration::from_secs(
            options.connect_timeout_secs.max(1),
        ))
        .read_timeout(std::time::Duration::from_secs(
            options.read_timeout_secs.max(1),
        ))
        .user_agent(
            options
                .user_agent
                .clone()
                .unwrap_or_else(|| ariaflow_core::settings::DEFAULT_USER_AGENT.to_string()),
        )
        .build()
        .map_err(|e| ProviderError::Network(format!("failed to build HTTP client: {e}")))?;
    let resp = client
        .get(doc_url)
        .header("Accept", "application/metalink4+xml, application/metalink+xml, application/xml, text/xml, text/plain")
        .send()
        .await
        .map_err(|e| ProviderError::Network(format!("fetch metalink document: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(ProviderError::BadStatus {
            status: status.as_u16(),
            message: format!("metalink document GET {doc_url} failed"),
        });
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ProviderError::Network(format!("read metalink document: {e}")))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(ProviderError::Unsupported(
            "metalink document exceeds 4 MiB".into(),
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Extracts the first `<file ...>...</file>` block (or the body up to the first
/// closing tag after `<file`) and parses its fields.
fn parse_metalink(xml: &str) -> Result<MetalinkFile, ProviderError> {
    // Find the first file section.
    let file_start = xml.find("<file").ok_or_else(|| {
        ProviderError::Unsupported("metalink document has no <file> element".into())
    })?;
    let rest = &xml[file_start..];
    let body_end = rest.find("</file>").unwrap_or(rest.len());
    let body = &rest[..body_end];

    let mut out = MetalinkFile::default();

    // name attribute on <file name="...">
    if let Some(tag_end) = body.find('>') {
        let tag = &body[..tag_end];
        out.name = attr(tag, "name");
    }

    out.size = tag_text(body, "size").and_then(|s| s.trim().parse::<u64>().ok());

    // <hash type="sha256">HEX</hash>
    for (ty, digest) in iter_tagged(body, "hash") {
        if !digest.trim().is_empty() {
            out.hashes
                .push((ty.to_ascii_lowercase(), digest.trim().to_string()));
        }
    }

    // <url preference="1">https://...</url> (preference optional, lower = better)
    for (pref, url) in iter_tagged_attr(body, "url", "preference") {
        let url = url.trim().to_string();
        if !url.is_empty() {
            let preference = pref
                .and_then(|p| p.trim().parse::<u32>().ok())
                .unwrap_or(100);
            out.mirrors.push((preference, url));
        }
    }

    if out.mirrors.is_empty() {
        return Err(ProviderError::Unsupported(
            "metalink document lists no mirror <url>".into(),
        ));
    }
    Ok(out)
}

/// Reads a `name="value"` (or `name='value'`) attribute inside a tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pattern = format!("{name}={quote}");
        if let Some(start) = tag.find(&pattern) {
            let rest = &tag[start + pattern.len()..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

fn tag_text(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = body.find(&open)?;
    let after = &body[start + open.len()..];
    let gt = after.find('>')?;
    let content = &after[gt + 1..];
    let end = content.find(&close)?;
    Some(content[..end].to_string())
}

fn iter_tagged(body: &str, tag: &str) -> Vec<(String, String)> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find(&open) {
        let after_open = &rest[start + open.len()..];
        let Some(gt) = after_open.find('>') else {
            break;
        };
        let tag_attrs = &after_open[..gt];
        let content = &after_open[gt + 1..];
        let Some(end) = content.find(&close) else {
            break;
        };
        let ty = attr(tag_attrs, "type").unwrap_or_else(|| "unknown".to_string());
        out.push((ty, content[..end].to_string()));
        rest = &content[end + close.len()..];
    }
    out
}

fn iter_tagged_attr(body: &str, tag: &str, attr_name: &str) -> Vec<(Option<String>, String)> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find(&open) {
        let after_open = &rest[start + open.len()..];
        let Some(gt) = after_open.find('>') else {
            break;
        };
        let tag_attrs = &after_open[..gt];
        let content = &after_open[gt + 1..];
        let Some(end) = content.find(&close) else {
            break;
        };
        out.push((attr(tag_attrs, attr_name), content[..end].to_string()));
        rest = &content[end + close.len()..];
    }
    out
}

fn select_mirror(mirrors: &[(u32, String)]) -> Option<&str> {
    mirrors
        .iter()
        .min_by_key(|(pref, url)| (*pref, url.len()))
        .map(|(_, url)| url.as_str())
}

fn hash_header(algorithm: &str) -> Option<&'static str> {
    match algorithm.to_ascii_lowercase().as_str() {
        "md5" => Some("x-checksum-md5"),
        "sha1" | "sha-1" => Some("x-checksum-sha1"),
        "sha256" | "sha-256" => Some("x-checksum-sha256"),
        "sha512" | "sha-512" => Some("x-checksum-sha512"),
        "blake3" => Some("x-checksum-blake3"),
        _ => None,
    }
}

#[async_trait]
impl DownloadProvider for MetalinkProvider {
    fn schemes(&self) -> &'static [&'static str] {
        &["metalink"]
    }

    fn supports_resume(&self) -> bool {
        true // resume happens transparently on the selected mirror (Range)
    }

    async fn probe(
        &self,
        url: &str,
        options: &ProviderOptions,
    ) -> Result<RemoteFileInfo, ProviderError> {
        let doc_url = document_url(url)?;
        let xml = fetch_document(&doc_url, options).await?;
        let file = parse_metalink(&xml)?;
        let mirror = select_mirror(&file.mirrors)
            .ok_or_else(|| ProviderError::Unsupported("no usable mirror".into()))?
            .to_string();

        // Probe the selected mirror; if it fails, degrade to metalink-only info.
        let probed = self.http.probe(&mirror, options).await.ok();
        let total_size = file.size.or(probed.as_ref().and_then(|p| p.total_size));
        let accept_ranges = probed.as_ref().map(|p| p.accept_ranges).unwrap_or(false);
        let filename_hint = file
            .name
            .clone()
            .map(|n| ariaflow_core::filename::sanitize_filename(&n))
            .or(probed.as_ref().and_then(|p| p.filename_hint.clone()));

        let mut headers: Vec<(String, String)> = probed
            .as_ref()
            .map(|p| p.headers.clone())
            .unwrap_or_default();
        for (algorithm, digest) in &file.hashes {
            if let Some(name) = hash_header(algorithm) {
                headers.push((name.to_string(), digest.clone()));
            }
        }

        Ok(RemoteFileInfo {
            final_url: mirror,
            total_size,
            accept_ranges,
            etag: probed.as_ref().and_then(|p| p.etag.clone()),
            last_modified: probed.as_ref().and_then(|p| p.last_modified.clone()),
            content_type: probed.as_ref().and_then(|p| p.content_type.clone()),
            filename_hint,
            headers,
        })
    }

    async fn download(
        &self,
        url: &str,
        options: &ProviderOptions,
        dest_part: &std::path::Path,
        progress: Arc<dyn Fn(TransferProgress) + Send + Sync>,
        should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<TransferOutcome, ProviderError> {
        let doc_url = document_url(url)?;
        let xml = fetch_document(&doc_url, options).await?;
        let file = parse_metalink(&xml)?;
        let mirror = select_mirror(&file.mirrors)
            .ok_or_else(|| ProviderError::Unsupported("no usable mirror".into()))?
            .to_string();
        tracing::info!(mirror = %mirror, "metalink: delegating payload transfer to mirror");
        self.http
            .download(&mirror, options, dest_part, progress, should_abort)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metalink4() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="release.iso">
    <size>3145851</size>
    <hash type="sha256">deadbeef</hash>
    <url preference="2">https://mirror2.example.com/release.iso</url>
    <url preference="1">https://mirror1.example.com/release.iso</url>
  </file>
</metalink>"#;
        let f = parse_metalink(xml).unwrap();
        assert_eq!(f.name.as_deref(), Some("release.iso"));
        assert_eq!(f.size, Some(3145851));
        assert_eq!(
            f.hashes,
            vec![("sha256".to_string(), "deadbeef".to_string())]
        );
        assert_eq!(
            select_mirror(&f.mirrors),
            Some("https://mirror1.example.com/release.iso")
        );
    }

    #[test]
    fn parses_metalink3() {
        let xml = r#"<metalinks><files><file name="a.tar.gz">
  <size>42</size><hash md5="xx" type="md5">cafe</hash>
  <url>https://only.example.com/a.tar.gz</url>
</file></files></metalinks>"#;
        let f = parse_metalink(xml).unwrap();
        assert_eq!(f.size, Some(42));
        assert_eq!(f.mirrors.len(), 1);
    }

    #[test]
    fn rejects_document_without_mirrors() {
        assert!(
            parse_metalink("<metalink><file name=\"x\"><size>1</size></file></metalink>").is_err()
        );
    }

    #[test]
    fn metalink_document_url_roundtrip() {
        assert_eq!(
            document_url("metalink:https://example.com/a.metalink").unwrap(),
            "https://example.com/a.metalink"
        );
        assert!(document_url("metalink:ftp://example.com/a").is_err());
    }
}
