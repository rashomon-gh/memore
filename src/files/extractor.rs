//! Text extraction from PDF and Markdown documents.

use anyhow::{Result, Context};
use lopdf::Document;
use pulldown_cmark::{Parser, Event, Tag, TagEnd, HeadingLevel};
use std::path::Path;

use crate::files::{FileType, ExtractedContent, ContentMetadata, CodeBlock};

/// Extract text content from a file.
pub fn extract_content(file_path: &Path, file_type: &FileType) -> Result<ExtractedContent> {
    match file_type {
        FileType::PDF => extract_pdf_text(file_path),
        FileType::Markdown => extract_markdown_text(file_path),
        FileType::Text => extract_plain_text(file_path),
        FileType::Unknown => Err(anyhow::anyhow!("Unknown file type")),
    }
}

/// Extract text from PDF file.
fn extract_pdf_text(file_path: &Path) -> Result<ExtractedContent> {
    let doc = Document::load(file_path)
        .context("Failed to load PDF file")?;

    // Extract all pages from the PDF
    let pages: Vec<u32> = doc.get_pages().keys().cloned().collect();
    let text = doc.extract_text(&pages)
        .context("Failed to extract text from PDF")?;

    // Simplified metadata extraction (lopdf 0.34 doesn't have easy metadata access)
    let metadata = ContentMetadata {
        title: None,
        author: None,
        created_date: None,
        language: None,
    };

    Ok(ExtractedContent {
        text,
        metadata,
        code_blocks: Vec::new(),
    })
}

/// Extract text and structured content from Markdown file.
fn extract_markdown_text(file_path: &Path) -> Result<ExtractedContent> {
    let content = std::fs::read_to_string(file_path)
        .context("Failed to read Markdown file")?;

    let parser = Parser::new(&content);

    let mut text = String::new();
    let mut in_code_block = false;
    let mut code_language = None;
    let mut code_start = 0;
    let mut code_content = String::new();
    let mut code_blocks = Vec::new();
    let mut line_number: usize = 0;

    let mut title = None;
    let mut current_section = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }) => {
                current_section.clear();
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => {
                if title.is_none() {
                    title = Some(current_section.clone());
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_start = line_number;
                code_language = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        Some(lang.to_string())
                    }
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };
                code_content.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                if in_code_block {
                    code_blocks.push(CodeBlock {
                        language: code_language.clone(),
                        code: code_content.clone(),
                        line_start: code_start,
                        line_end: line_number,
                    });
                    in_code_block = false;
                }
            }
            Event::Text(t) => {
                let t_str = t.to_string();
                if in_code_block {
                    code_content.push_str(&t_str);
                    code_content.push('\n');
                } else {
                    text.push_str(&t_str);
                    current_section.push_str(&t_str);
                }
                line_number += t_str.matches('\n').count();
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block {
                    code_content.push('\n');
                } else {
                    text.push('\n');
                    current_section.push(' ');
                }
                line_number += 1;
            }
            _ => {}
        }
    }

    Ok(ExtractedContent {
        text,
        metadata: ContentMetadata {
            title,
            author: None,
            created_date: None,
            language: Some("markdown".to_string()),
        },
        code_blocks,
    })
}

/// Extract text from plain text file.
fn extract_plain_text(file_path: &Path) -> Result<ExtractedContent> {
    let text = std::fs::read_to_string(file_path)
        .context("Failed to read text file")?;

    Ok(ExtractedContent {
        text,
        metadata: ContentMetadata {
            title: None,
            author: None,
            created_date: None,
            language: None,
        },
        code_blocks: Vec::new(),
    })
}

/// Detect file type from extension.
pub fn detect_file_type(file_path: &Path) -> FileType {
    file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| match ext.to_lowercase().as_str() {
            "pdf" => FileType::PDF,
            "md" | "markdown" => FileType::Markdown,
            "txt" | "text" => FileType::Text,
            _ => FileType::Unknown,
        })
        .unwrap_or(FileType::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_file_type() {
        assert_eq!(detect_file_type(Path::new("test.pdf")), FileType::PDF);
        assert_eq!(detect_file_type(Path::new("test.md")), FileType::Markdown);
        assert_eq!(detect_file_type(Path::new("test.txt")), FileType::Text);
        assert_eq!(detect_file_type(Path::new("test.unknown")), FileType::Unknown);
    }
}
