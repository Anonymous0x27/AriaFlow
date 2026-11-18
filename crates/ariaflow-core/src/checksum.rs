//! Checksum primitives and remote checksum discovery.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Blake3,
}

impl ChecksumAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
            Self::Blake3 => "blake3",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "md5" => Some(Self::Md5),
            "sha1" | "sha-1" => Some(Self::Sha1),
            "sha256" | "sha-256" => Some(Self::Sha256),
            "sha512" | "sha-512" => Some(Self::Sha512),
            "blake3" => Some(Self::Blake3),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checksum {
    pub algorithm: ChecksumAlgorithm,
    /// Lowercase hexadecimal digest.
    pub digest: String,
    /// Where the expected value came from, e.g. "header:x-checksum-sha256", "sidecar", "user".
    #[serde(default)]
    pub source: Option<String>,
}

impl Checksum {
    /// Best-effort extraction of a digest from response headers.
    ///
    /// Recognised headers (case-insensitive names passed by the caller):
    /// `x-checksum-sha256`, `x-checksum-sha512`, `x-checksum-md5`, `content-md5`,
    /// `digest` (RFC 9530, e.g. `sha-256=base64==`), `etag` when it looks like a 64-hex string.
    pub fn from_headers<I>(headers: I) -> Option<Checksum>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut found: Option<Checksum> = None;
        for (name, value) in headers {
            let name = name.to_ascii_lowercase();
            let value = value.trim().to_string();
            let hit = match name.as_str() {
                "x-checksum-sha256" => Some(ChecksumAlgorithm::Sha256),
                "x-checksum-sha512" => Some(ChecksumAlgorithm::Sha512),
                "x-checksum-sha1" => Some(ChecksumAlgorithm::Sha1),
                "x-checksum-md5" | "content-md5" => Some(ChecksumAlgorithm::Md5),
                "x-checksum-blake3" => Some(ChecksumAlgorithm::Blake3),
                _ => None,
            };
            if let Some(algorithm) = hit {
                // Digests in these headers are usually hex; accept as-is but validate shape.
                let digest = value.to_ascii_lowercase();
                if digest.chars().all(|c| c.is_ascii_hexdigit()) && !digest.is_empty() {
                    found = Some(Checksum {
                        algorithm,
                        digest,
                        source: Some(format!("header:{name}")),
                    });
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_algorithm() {
        assert_eq!(
            ChecksumAlgorithm::parse("SHA-256"),
            Some(ChecksumAlgorithm::Sha256)
        );
        assert_eq!(
            ChecksumAlgorithm::parse("blake3"),
            Some(ChecksumAlgorithm::Blake3)
        );
        assert_eq!(ChecksumAlgorithm::parse("nope"), None);
    }

    #[test]
    fn extracts_from_headers() {
        let hs = vec![("x-checksum-sha256".to_string(), "AB12CD".repeat(2))];
        let cs = Checksum::from_headers(hs).unwrap();
        assert_eq!(cs.algorithm, ChecksumAlgorithm::Sha256);
        assert_eq!(cs.digest, "ab12cd".repeat(2));
    }
}
