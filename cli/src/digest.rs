use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::Digest;

/// A validated SHA-256 digest, stored as raw bytes.
///
/// Serializes to/from a lowercase hex string for JSON/TOML.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// Finalize a `sha2::Sha256` hasher into a digest.
    pub fn from_hasher(hasher: sha2::Sha256) -> Self {
        let arr: [u8; 32] = hasher.finalize().into();
        Self(arr)
    }

    /// Parse from a hex string (64 lowercase hex chars).
    pub fn from_hex(s: &str) -> anyhow::Result<Self> {
        let s = s.trim();
        if s.len() != 64 {
            anyhow::bail!("SHA-256 hex string must be 64 characters, got {}", s.len());
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hi = hex_digit(chunk[0])?;
            let lo = hex_digit(chunk[1])?;
            bytes[i] = (hi << 4) | lo;
        }
        Ok(Self(bytes))
    }
}

fn hex_digit(b: u8) -> anyhow::Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => anyhow::bail!("invalid hex digit: {:?}", b as char),
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256Digest({self})")
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.is_empty() {
            // Allow empty string for backwards compat with default state
            return Ok(Self([0u8; 32]));
        }
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_hasher() {
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"hello");
        let digest = Sha256Digest::from_hasher(hasher);
        assert_eq!(
            digest.to_string(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_from_hex_roundtrip() {
        let hex = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let digest = Sha256Digest::from_hex(hex).unwrap();
        assert_eq!(digest.to_string(), hex);
    }

    #[test]
    fn test_from_hex_invalid_length() {
        assert!(Sha256Digest::from_hex("abc").is_err());
    }

    #[test]
    fn test_from_hex_invalid_chars() {
        let bad = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert!(Sha256Digest::from_hex(bad).is_err());
    }

    #[test]
    fn test_serde_roundtrip() {
        let hex = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let digest = Sha256Digest::from_hex(hex).unwrap();
        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(json, format!("\"{hex}\""));
        let parsed: Sha256Digest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, digest);
    }

    #[test]
    fn test_default_is_zeroed() {
        let d = Sha256Digest::default();
        assert_eq!(
            d.to_string(),
            "0000000000000000000000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn test_deserialize_empty_string() {
        let parsed: Sha256Digest = serde_json::from_str("\"\"").unwrap();
        assert_eq!(parsed, Sha256Digest::default());
    }
}
