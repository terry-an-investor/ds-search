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

// ═══════════════════════════════════════════════════════════
// Conversation extraction
// ═══════════════════════════════════════════════════════════

/// Role of a chat turn in an AI Studio conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnRole {
    User,
    Model,
}

impl TurnRole {
    pub fn as_label(&self) -> &'static str {
        match self {
            TurnRole::User => "user",
            TurnRole::Model => "model",
        }
    }
}

/// A single chat turn (user prompt or model response).
#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: TurnRole,
    pub content: String,
}

/// A full AI Studio conversation extracted from a /prompts/<id> page.
#[derive(Debug, Clone)]
pub struct Conversation {
    pub title: String,
    pub url: String,
    pub turns: Vec<ChatTurn>,
}

// ═══════════════════════════════════════════════════════════
// Playground tools
// ═══════════════════════════════════════════════════════════

/// Toggleable Playground tools (knowledge/aistudio.google.com.yaml §2).
///
/// `as_label()` returns the substring used to match the tool's button text;
/// `from_label()` accepts the short CLI name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// "Structured outputs" toggle.
    StructuredOutputs,
    /// "Code execution" toggle.
    CodeExecution,
    /// "Function calling" toggle.
    FunctionCalling,
    /// "Grounding with Google Search".
    GoogleSearchGrounding,
    /// "Grounding with Google Maps".
    MapsGrounding,
    /// "URL context" toggle.
    UrlContext,
}

impl Tool {
    /// Button-text substring used to locate the tool on the page.
    pub fn as_label(&self) -> &'static str {
        match self {
            Tool::StructuredOutputs => "Structured outputs",
            Tool::CodeExecution => "Code execution",
            Tool::FunctionCalling => "Function calling",
            Tool::GoogleSearchGrounding => "Grounding with Google Search",
            Tool::MapsGrounding => "Grounding with Google Maps",
            Tool::UrlContext => "URL context",
        }
    }

    /// Parse from CLI argument (case-insensitive).
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "structured" | "structured-outputs" | "structuredoutputs" => {
                Some(Tool::StructuredOutputs)
            }
            "code" | "code-execution" => Some(Tool::CodeExecution),
            "function" | "function-calling" => Some(Tool::FunctionCalling),
            "search" | "google-search" | "grounding" => Some(Tool::GoogleSearchGrounding),
            "maps" | "google-maps" => Some(Tool::MapsGrounding),
            "url" | "url-context" | "urlcontext" => Some(Tool::UrlContext),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════
// Thinking level (typed)
// ═══════════════════════════════════════════════════════════

/// Reasoning depth for models that support it (Low/Medium/High).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingLevel {
    Low,
    Medium,
    High,
}

impl ThinkingLevel {
    pub fn as_label(&self) -> &'static str {
        match self {
            ThinkingLevel::Low => "Low",
            ThinkingLevel::Medium => "Medium",
            ThinkingLevel::High => "High",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(ThinkingLevel::Low),
            "medium" => Some(ThinkingLevel::Medium),
            "high" => Some(ThinkingLevel::High),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════
// Lightweight page state & response stats
// ═══════════════════════════════════════════════════════════

/// Single-eval snapshot of the playground page (mirrors grok::get_state).
#[derive(Debug, Clone, Default)]
pub struct AistudioState {
    pub url: String,
    pub is_on_playground: bool,
    pub has_input: bool,
    pub is_streaming: bool,
    pub user_turn_count: usize,
    pub model_turn_count: usize,
    pub current_model: String,
}

/// Metrics shown after a model turn: runtime pill + token count.
#[derive(Debug, Clone, Default)]
pub struct PromptStats {
    /// e.g. "3.9s" from `.model-run-time-pill`, if present.
    pub runtime: Option<String>,
    /// Token-count string if surfaced on the page.
    pub token_count: Option<String>,
}
