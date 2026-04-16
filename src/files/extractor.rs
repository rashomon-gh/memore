//! Text extraction from PDF and Markdown documents.

use anyhow::{Result, Context};
use lopdf::Document;
use pulldown_cmark::{Parser, Event, Tag, TagEnd, HeadingLevel};
use std::path::Path;
use std::fs;

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
    // Try lopdf first
    let lopdf_result = extract_with_lopdf(file_path);

    if let Ok(text) = lopdf_result {
        if !text.trim().is_empty() {
            return Ok(ExtractedContent {
                text,
                metadata: ContentMetadata {
                    title: None,
                    author: None,
                    created_date: None,
                    language: None,
                },
                code_blocks: Vec::new(),
            });
        }
    }

    // Fallback to pdf-extract if lopdf failed or returned empty text
    tracing::info!("lopdf extraction failed or returned empty text, trying pdf-extract");
    extract_with_pdf_extract(file_path)
}

/// Extract text using lopdf library.
fn extract_with_lopdf(file_path: &Path) -> Result<String> {
    let doc = Document::load(file_path)
        .context("Failed to load PDF file with lopdf")?;

    let mut text = String::new();

    // Try to get pages and extract text
    let pages = doc.get_pages();
    if pages.is_empty() {
        // If no pages found, try alternative extraction method
        text = doc.extract_text(&[])
            .unwrap_or_else(|_| String::new());
    } else {
        // Extract text from each page individually for better error handling
        let page_ids: Vec<u32> = pages.keys().cloned().collect();
        let mut sorted_pages = page_ids.clone();
        sorted_pages.sort();

        for (index, page_id) in sorted_pages.iter().enumerate() {
            match doc.extract_text(&[*page_id]) {
                Ok(page_text) => {
                    if !page_text.trim().is_empty() {
                        if !text.is_empty() {
                            text.push_str("\n\n--- Page ");
                            text.push_str(&(index + 1).to_string());
                            text.push_str(" ---\n\n");
                        }
                        text.push_str(&page_text);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to extract text from page {}: {}", page_id, e);
                    // Continue with other pages instead of failing completely
                }
            }
        }
    }

    Ok(text)
}

/// Extract text using pdf-extract library (fallback).
fn extract_with_pdf_extract(file_path: &Path) -> Result<ExtractedContent> {
    let pdf_data = fs::read(file_path)
        .context("Failed to read PDF file")?;

    match pdf_extract::extract_text_from_mem(&pdf_data) {
        Ok(text) => {
            if text.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "PDF extraction failed. The PDF might be:\n\
                     • Password-protected\n\
                     • Contains images instead of text (scanned document)\n\
                     • Uses an unsupported encoding format\n\
                     • Corrupted or invalid\n\n\
                     Please try converting the PDF to a searchable format or use a Markdown file instead."
                ));
            }

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
        Err(e) => {
            Err(anyhow::anyhow!(
                "PDF extraction failed with both lopdf and pdf-extract libraries.\n\
                 Error details: {}\n\n\
                 This PDF might be password-protected, scanned (images only), or use an incompatible format.\n\
                 Please try:\n\
                 • Converting the PDF to a searchable format\n\
                 • Using a Markdown version instead\n\
                 • Running OCR on scanned documents first", e
            ))
        }
    }
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
