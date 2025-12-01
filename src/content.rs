// src/content.rs
// Content extraction data structures

#[derive(Debug)]
pub struct ContentEntry {
    pub source_node: String,
    pub breadcrumb_path: String,
    pub content_type: String,
    pub url: String,
    pub title: String,
    pub author: String,
    pub file_type: String,
    pub size: String,
    pub url_valid: String,
}

impl ContentEntry {
    /// Create a new ContentEntry with basic fields
    pub fn new(source_node: String, breadcrumb_path: String, content_type: String) -> Self {
        Self {
            source_node,
            breadcrumb_path,
            content_type,
            url: String::new(),
            title: String::new(),
            author: String::new(),
            file_type: String::new(),
            size: String::new(),
            url_valid: String::new(),
        }
    }
}
