//! Data models for WeRead.

/// Book metadata from Weread.
#[derive(Debug, Clone)]
pub struct BookInfo {
    pub book_id: String,      // short ID used in URLs
    pub long_book_id: String, // long ID used in APIs
    pub title: String,
    pub author: String,
    pub cover: String,
    pub publisher: String,
    pub format: String, // "epub" or "pdf"
    pub price: f64,
    pub rating: u32, // star * 100 (e.g., 96 = 96%)
    pub rating_count: u32,
    pub intro: String, // book description
    pub total_chapters: u32,
}

/// A single chapter from the table of contents.
#[derive(Debug, Clone)]
pub struct ChapterInfo {
    pub chapter_uid: u32,
    pub chapter_idx: u32, // 1-based position
    pub title: String,
    pub word_count: u32,
    pub level: u32,  // 1=chapter, 2=section, 3=subsection
    pub price: u32,  // 0=free
    pub paid: u32,   // 0=not paid
    pub tar: String, // optional tar URL for content
}
