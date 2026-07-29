use serde::{Deserialize, Serialize};

use crate::formats::csv::CsvAnalysis;
use crate::formats::txt::TxtAnalysis;

pub const CURRENT_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileType {
    Csv,
    Txt,
    Unknown,
}

impl FileType {
    pub fn from_path(path: &std::path::Path) -> Self {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("csv") => Self::Csv,
            Some("txt") | Some("log") => Self::Txt,
            _ => Self::Unknown,
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Self::Csv => 1,
            Self::Txt => 2,
            Self::Unknown => 255,
        }
    }

    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Csv),
            2 => Some(Self::Txt),
            255 => Some(Self::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpackMetadata {
    pub version: u16,
    pub original_file_type: FileType,
    pub original_size: u64,
    pub payload_kind: PayloadKind,
    pub csv: Option<CsvAnalysis>,
    pub txt: Option<TxtAnalysis>,
    pub future_extensions: Vec<ExtensionPoint>,
}

impl DpackMetadata {
    pub fn new(original_file_type: FileType, original_size: u64) -> Self {
        Self {
            version: CURRENT_VERSION,
            original_file_type,
            original_size,
            payload_kind: PayloadKind::RawZstd,
            csv: None,
            txt: None,
            future_extensions: vec![
                ExtensionPoint::new("json", false),
                ExtensionPoint::new("toon", false),
                ExtensionPoint::new("gpu", false),
                ExtensionPoint::new("ssd_cache", false),
                ExtensionPoint::new("ai_ml_models", false),
            ],
        }
    }

    pub fn minimal(original_file_type: FileType, original_size: u64) -> Self {
        Self {
            version: CURRENT_VERSION,
            original_file_type,
            original_size,
            payload_kind: PayloadKind::RawZstd,
            csv: None,
            txt: None,
            future_extensions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PayloadKind {
    RawZstd,
    CsvColumnarDictionary,
    Plain,
    Dictionary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionPoint {
    pub name: String,
    pub enabled: bool,
}

impl ExtensionPoint {
    pub fn new(name: impl Into<String>, enabled: bool) -> Self {
        Self {
            name: name.into(),
            enabled,
        }
    }
}
