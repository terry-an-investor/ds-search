//! Data models for Google AI Studio.

/// Available Gemini models on AI Studio.
#[derive(Debug, Clone, PartialEq)]
pub enum AistudioModel {
    /// Gemini 3.1 Flash Lite — most cost-efficient
    FlashLite,
    /// Gemini 3 Flash Preview — fast + intelligent, good search/grounding
    Flash,
    /// Gemini 3.1 Pro Preview — SOTA reasoning, deep/nuanced
    Pro,
    /// Gemini 3.1 Flash Image Preview (Nano Banana 2)
    FlashImage,
    /// Gemini 3 Pro Image Preview (Nano Banana Pro)
    ProImage,
    /// Gemini Pro Latest (alias to Pro)
    ProLatest,
    /// Gemini Flash Latest (alias to Flash)
    FlashLatest,
    /// Gemini Flash-Lite Latest (alias to FlashLite)
    FlashLiteLatest,
}

impl AistudioModel {
    /// Model ID string used in the API / UI.
    pub fn model_id(&self) -> &'static str {
        match self {
            AistudioModel::FlashLite => "gemini-3.1-flash-lite",
            AistudioModel::Flash => "gemini-3-flash-preview",
            AistudioModel::Pro => "gemini-3.1-pro-preview",
            AistudioModel::FlashImage => "gemini-3.1-flash-image-preview",
            AistudioModel::ProImage => "gemini-3-pro-image-preview",
            AistudioModel::ProLatest => "gemini-pro-latest",
            AistudioModel::FlashLatest => "gemini-flash-latest",
            AistudioModel::FlashLiteLatest => "gemini-flash-lite-latest",
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            AistudioModel::FlashLite => "Gemini 3.1 Flash Lite",
            AistudioModel::Flash => "Gemini 3 Flash Preview",
            AistudioModel::Pro => "Gemini 3.1 Pro Preview",
            AistudioModel::FlashImage => "Gemini 3.1 Flash Image (Nano Banana 2)",
            AistudioModel::ProImage => "Gemini 3 Pro Image (Nano Banana Pro)",
            AistudioModel::ProLatest => "Gemini Pro Latest",
            AistudioModel::FlashLatest => "Gemini Flash Latest",
            AistudioModel::FlashLiteLatest => "Gemini Flash-Lite Latest",
        }
    }

    /// Parse from CLI argument (case-insensitive).
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "flash-lite" | "flashlite" | "gemini-3.1-flash-lite" => Some(AistudioModel::FlashLite),
            "flash" | "gemini-3-flash-preview" => Some(AistudioModel::Flash),
            "pro" | "gemini-3.1-pro-preview" => Some(AistudioModel::Pro),
            "flash-image" | "nano-banana-2" | "gemini-3.1-flash-image-preview" => {
                Some(AistudioModel::FlashImage)
            }
            "pro-image" | "nano-banana-pro" | "gemini-3-pro-image-preview" => {
                Some(AistudioModel::ProImage)
            }
            "pro-latest" | "gemini-pro-latest" => Some(AistudioModel::ProLatest),
            "flash-latest" | "gemini-flash-latest" => Some(AistudioModel::FlashLatest),
            "flash-lite-latest" | "gemini-flash-lite-latest" => {
                Some(AistudioModel::FlashLiteLatest)
            }
            _ => None,
        }
    }
}
