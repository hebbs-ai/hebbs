use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{Result, VaultError};

/// Result of parsing a single `.md` file.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// Parsed YAML frontmatter (None if no frontmatter present).
    pub frontmatter: Option<HashMap<String, serde_yaml::Value>>,
    /// Sections extracted from the file body.
    pub sections: Vec<ParsedSection>,
}

/// A single section extracted from a markdown file.
#[derive(Debug, Clone)]
pub struct ParsedSection {
    /// Hierarchical heading path, e.g., ["Design", "API"] for `## Design` > `### API`.
    pub heading_path: Vec<String>,
    /// Heading level: 0 = preamble (before first heading), 1 = `#`, 2 = `##`, etc.
    pub heading_level: u8,
    /// The prose content under this heading (excluding the heading line itself).
    pub content: String,
    /// Byte offset of section start relative to file start.
    pub byte_start: usize,
    /// Byte offset of section end relative to file start.
    pub byte_end: usize,
    /// Wiki-links found in this section.
    pub wiki_links: Vec<WikiLink>,
    /// Tags found in this section.
    pub tags: Vec<String>,
}

/// A wiki-link extracted from markdown: `[[target]]`, `[[target|alias]]`, `[[target#section]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiLink {
    /// Target file path or note name.
    pub target: String,
    /// Display alias text.
    pub alias: Option<String>,
    /// Section anchor within the target.
    pub section: Option<String>,
}

/// Parse a markdown file from bytes.
///
/// Splits on headings at `split_level` (e.g., 2 for `##`).
/// Returns structured sections with byte offsets, wiki-links, and tags.
///
/// Time complexity: O(n) where n = file size in bytes.
pub fn parse_markdown(input: &[u8], split_level: usize) -> Result<ParsedFile> {
    let text = std::str::from_utf8(input).map_err(|e| VaultError::Parse {
        path: "<bytes>".into(),
        reason: format!("invalid UTF-8: {e}"),
    })?;

    parse_markdown_str(text, split_level)
}

/// Parse a markdown file from a file path.
pub fn parse_markdown_file(path: &Path, split_level: usize) -> Result<ParsedFile> {
    let bytes = std::fs::read(path).map_err(|e| VaultError::Parse {
        path: path.to_path_buf(),
        reason: format!("failed to read: {e}"),
    })?;

    let text = std::str::from_utf8(&bytes).map_err(|e| VaultError::Parse {
        path: path.to_path_buf(),
        reason: format!("invalid UTF-8: {e}"),
    })?;

    parse_markdown_str(text, split_level)
}

fn parse_markdown_str(text: &str, split_level: usize) -> Result<ParsedFile> {
    let (frontmatter, body_start) = extract_frontmatter(text);
    let sections = extract_sections(text, body_start, split_level);
    Ok(ParsedFile {
        frontmatter,
        sections,
    })
}

/// Extract YAML frontmatter from the beginning of a markdown file.
/// Returns (parsed frontmatter, byte offset where body starts).
fn extract_frontmatter(text: &str) -> (Option<HashMap<String, serde_yaml::Value>>, usize) {
    if !text.starts_with("---") {
        return (None, 0);
    }

    // Find the closing ---
    let after_opener = &text[3..];
    // Must have a newline after opening ---
    let newline_pos = match after_opener.find('\n') {
        Some(pos) => pos,
        None => return (None, 0),
    };

    // Check that opening --- is only followed by optional whitespace then newline
    let opener_rest = &after_opener[..newline_pos];
    if !opener_rest.trim().is_empty() {
        return (None, 0);
    }

    let content_start = 3 + newline_pos + 1; // after "---\n"
    let remaining = &text[content_start..];

    // Find closing --- on its own line
    let mut search_pos = 0;
    loop {
        let line_start = search_pos;
        let line_end = remaining[search_pos..]
            .find('\n')
            .map(|p| search_pos + p)
            .unwrap_or(remaining.len());
        let line = remaining[line_start..line_end].trim();

        if line == "---" {
            let yaml_str = &remaining[..line_start];
            let body_start = content_start + line_end;
            // Skip the newline after closing ---
            let body_start =
                if body_start < text.len() && text.as_bytes().get(body_start) == Some(&b'\n') {
                    body_start + 1
                } else {
                    body_start
                };

            match serde_yaml::from_str(yaml_str) {
                Ok(map) => return (Some(map), body_start),
                Err(_) => {
                    // Invalid YAML: treat as no frontmatter, log warning
                    tracing::warn!("invalid YAML frontmatter, treating as body content");
                    return (None, 0);
                }
            }
        }

        if line_end >= remaining.len() {
            // No closing --- found
            return (None, 0);
        }
        search_pos = line_end + 1;
    }
}

/// Extract sections from the body of a markdown file, splitting on headings.
fn extract_sections(text: &str, body_start: usize, split_level: usize) -> Vec<ParsedSection> {
    let body = &text[body_start..];
    if body.trim().is_empty() {
        return Vec::new();
    }

    let wiki_link_re =
        Regex::new(r"\[\[([^\]#|]+)(?:#([^\]|]+))?(?:\|([^\]]+))?\]\]").expect("valid regex");
    let tag_re = Regex::new(r"(?:^|[\s(])#([a-zA-Z][a-zA-Z0-9_/-]*)").expect("valid regex");

    // Find all heading positions in the body
    let mut heading_positions: Vec<(usize, u8, String)> = Vec::new(); // (byte_offset_in_body, level, title)

    let mut in_code_block = false;
    let mut line_start = 0;

    for line in body.lines() {
        let line_bytes = line.as_bytes();

        // Track fenced code blocks
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        }

        if !in_code_block {
            // Check if line is a heading
            let level = line_bytes.iter().take_while(|&&b| b == b'#').count();
            if level >= 1 && level <= 6 && line_bytes.get(level) == Some(&b' ') {
                let title = line[level..].trim().to_string();
                if !title.is_empty() {
                    heading_positions.push((line_start, level as u8, title));
                }
            }
        }

        line_start += line.len() + 1; // +1 for \n
    }

    // Build sections
    let mut sections = Vec::new();
    let mut heading_stack: Vec<(u8, String)> = Vec::new(); // (level, title)

    // Determine section boundaries
    // Only headings at or above split_level create new sections
    let split_positions: Vec<usize> = heading_positions
        .iter()
        .filter(|(_, level, _)| *level as usize <= split_level)
        .map(|(pos, _, _)| *pos)
        .collect();

    // If there's content before the first split heading, create a preamble section
    let first_split = split_positions.first().copied().unwrap_or(body.len());
    if first_split > 0 {
        let preamble = &body[..first_split];
        if !preamble.trim().is_empty() {
            let (wiki_links, tags) = extract_links_and_tags(preamble, &wiki_link_re, &tag_re);
            sections.push(ParsedSection {
                heading_path: Vec::new(),
                heading_level: 0,
                content: preamble.trim_end().to_string(),
                byte_start: body_start,
                byte_end: body_start + first_split,
                wiki_links,
                tags,
            });
        }
    }

    // Process each heading and its content
    for (idx, &(pos, level, ref title)) in heading_positions.iter().enumerate() {
        let is_split_heading = (level as usize) <= split_level;

        // Update heading stack
        if is_split_heading {
            // Pop everything at or below this level
            while heading_stack.last().map_or(false, |(l, _)| *l >= level) {
                heading_stack.pop();
            }
            heading_stack.push((level, title.clone()));
        }

        if !is_split_heading {
            // Sub-headings within a split section: they're part of the parent section content,
            // but we track them in the heading stack for path building
            // Update stack for nested headings
            while heading_stack.last().map_or(false, |(l, _)| *l >= level) {
                heading_stack.pop();
            }
            heading_stack.push((level, title.clone()));
            continue;
        }

        // Find the end of this section (next heading at split level, or end of body)
        let section_end = heading_positions
            .iter()
            .skip(idx + 1)
            .find(|(_, l, _)| (*l as usize) <= split_level)
            .map(|(p, _, _)| *p)
            .unwrap_or(body.len());

        let heading_path: Vec<String> = heading_stack.iter().map(|(_, t)| t.clone()).collect();

        let section_text = &body[pos..section_end];
        // Strip the heading line from content
        let content_start_in_section = section_text
            .find('\n')
            .map(|p| p + 1)
            .unwrap_or(section_text.len());
        let content = &section_text[content_start_in_section..];

        let (wiki_links, tags) = extract_links_and_tags(content, &wiki_link_re, &tag_re);

        sections.push(ParsedSection {
            heading_path,
            heading_level: level,
            content: content.trim_end().to_string(),
            byte_start: body_start + pos,
            byte_end: body_start + section_end,
            wiki_links,
            tags,
        });
    }

    // If no split headings found at all and no preamble was created, and body is not empty,
    // the preamble logic above already handles this.
    // But if there are only deeper headings and no preamble (body starts with a deep heading),
    // we need to handle that. The preamble check handles body[..first_split].
    if sections.is_empty() && !body.trim().is_empty() {
        // No headings at all, or none at split level
        let (wiki_links, tags) = extract_links_and_tags(body, &wiki_link_re, &tag_re);
        sections.push(ParsedSection {
            heading_path: Vec::new(),
            heading_level: 0,
            content: body.trim_end().to_string(),
            byte_start: body_start,
            byte_end: body_start + body.len(),
            wiki_links,
            tags,
        });
    }

    sections
}

/// Extract wiki-links and tags from a text block, respecting fenced code blocks.
fn extract_links_and_tags(
    text: &str,
    wiki_link_re: &Regex,
    tag_re: &Regex,
) -> (Vec<WikiLink>, Vec<String>) {
    let mut wiki_links = Vec::new();
    let mut tags = Vec::new();

    let mut in_code_block = false;
    let _in_inline_code = false;

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        // Process wiki-links (skip inline code spans)
        // Simple approach: remove inline code spans before matching
        let clean_line = remove_inline_code(line);

        for cap in wiki_link_re.captures_iter(&clean_line) {
            let target = cap[1].trim().to_string();
            let section = cap.get(2).map(|m| m.as_str().trim().to_string());
            let alias = cap.get(3).map(|m| m.as_str().trim().to_string());
            wiki_links.push(WikiLink {
                target,
                alias,
                section,
            });
        }

        for cap in tag_re.captures_iter(&clean_line) {
            let tag = cap[1].to_string();
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }

    (wiki_links, tags)
}

/// Remove inline code spans (backtick-delimited) from a line.
fn remove_inline_code(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut in_code = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '`' {
            in_code = !in_code;
            result.push(' '); // replace backtick with space to preserve word boundaries
        } else if in_code {
            result.push(' '); // replace code content with space
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_file_with_headings() {
        let input = b"## Introduction\n\nSome intro text.\n\n## Design\n\nDesign details here.\n";
        let result = parse_markdown(input, 2).unwrap();
        assert!(result.frontmatter.is_none());
        assert_eq!(result.sections.len(), 2);

        assert_eq!(result.sections[0].heading_path, vec!["Introduction"]);
        assert_eq!(result.sections[0].heading_level, 2);
        assert!(result.sections[0].content.contains("Some intro text."));

        assert_eq!(result.sections[1].heading_path, vec!["Design"]);
        assert_eq!(result.sections[1].heading_level, 2);
        assert!(result.sections[1].content.contains("Design details here."));
    }

    #[test]
    fn test_frontmatter_extraction() {
        let input =
            b"---\ntitle: Test Note\nhebbs-kind: insight\n---\n\n## Body\n\nContent here.\n";
        let result = parse_markdown(input, 2).unwrap();
        let fm = result.frontmatter.unwrap();
        assert_eq!(
            fm.get("title").unwrap(),
            &serde_yaml::Value::String("Test Note".to_string())
        );
        assert_eq!(
            fm.get("hebbs-kind").unwrap(),
            &serde_yaml::Value::String("insight".to_string())
        );
        assert_eq!(result.sections.len(), 1);
    }

    #[test]
    fn test_no_headings_single_section() {
        let input = b"Just some plain text without any headings.\n\nAnother paragraph.\n";
        let result = parse_markdown(input, 2).unwrap();
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].heading_level, 0);
        assert!(result.sections[0].heading_path.is_empty());
        assert!(result.sections[0].content.contains("plain text"));
    }

    #[test]
    fn test_empty_file() {
        let input = b"";
        let result = parse_markdown(input, 2).unwrap();
        assert!(result.frontmatter.is_none());
        assert!(result.sections.is_empty());
    }

    #[test]
    fn test_frontmatter_only() {
        let input = b"---\ntitle: Empty\n---\n";
        let result = parse_markdown(input, 2).unwrap();
        assert!(result.frontmatter.is_some());
        assert!(result.sections.is_empty());
    }

    #[test]
    fn test_nested_headings() {
        let input = b"## A\n\nA content.\n\n### B\n\nB content.\n\n## C\n\nC content.\n";
        let result = parse_markdown(input, 2).unwrap();
        // Split on ##, so ### B is inside ## A's section
        assert_eq!(result.sections.len(), 2);
        assert_eq!(result.sections[0].heading_path, vec!["A"]);
        assert!(result.sections[0].content.contains("A content."));
        assert!(result.sections[0].content.contains("### B")); // nested heading is part of content
        assert!(result.sections[0].content.contains("B content."));
        assert_eq!(result.sections[1].heading_path, vec!["C"]);
    }

    #[test]
    fn test_wiki_links() {
        let input = b"## Notes\n\nSee [[other-note]] and [[project#design|the design doc]].\n";
        let result = parse_markdown(input, 2).unwrap();
        let links = &result.sections[0].wiki_links;
        assert_eq!(links.len(), 2);

        assert_eq!(links[0].target, "other-note");
        assert!(links[0].alias.is_none());
        assert!(links[0].section.is_none());

        assert_eq!(links[1].target, "project");
        assert_eq!(links[1].section.as_deref(), Some("design"));
        assert_eq!(links[1].alias.as_deref(), Some("the design doc"));
    }

    #[test]
    fn test_tags() {
        let input = b"## Topic\n\nThis is about #design and #api/v2 stuff.\n";
        let result = parse_markdown(input, 2).unwrap();
        let tags = &result.sections[0].tags;
        assert!(tags.contains(&"design".to_string()));
        assert!(tags.contains(&"api/v2".to_string()));
    }

    #[test]
    fn test_code_block_headings_ignored() {
        let input = b"## Real Heading\n\nSome text.\n\n```\n## Fake Heading\n#fake-tag\n```\n\nMore text.\n";
        let result = parse_markdown(input, 2).unwrap();
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].heading_path, vec!["Real Heading"]);
        // Fake heading should not create a new section
        // Fake tag should not be extracted
        assert!(!result.sections[0].tags.contains(&"fake-tag".to_string()));
    }

    #[test]
    fn test_preamble_before_first_heading() {
        let input = b"Some preamble text.\n\n## First Heading\n\nContent under heading.\n";
        let result = parse_markdown(input, 2).unwrap();
        assert_eq!(result.sections.len(), 2);
        assert_eq!(result.sections[0].heading_level, 0);
        assert!(result.sections[0].content.contains("preamble text"));
        assert_eq!(result.sections[1].heading_path, vec!["First Heading"]);
    }

    #[test]
    fn test_heading_with_no_body() {
        let input = b"## Empty Section\n## Next Section\n\nSome content.\n";
        let result = parse_markdown(input, 2).unwrap();
        assert_eq!(result.sections.len(), 2);
        assert_eq!(result.sections[0].heading_path, vec!["Empty Section"]);
        assert!(result.sections[0].content.is_empty());
        assert_eq!(result.sections[1].heading_path, vec!["Next Section"]);
    }

    #[test]
    fn test_wiki_link_in_inline_code_ignored() {
        let input = b"## Test\n\nUse `[[not-a-link]]` in code, but [[real-link]] outside.\n";
        let result = parse_markdown(input, 2).unwrap();
        let links = &result.sections[0].wiki_links;
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "real-link");
    }

    #[test]
    fn test_byte_offsets_roundtrip() {
        let input = "---\ntitle: Test\n---\n\n## First\n\nContent A.\n\n## Second\n\nContent B.\n";
        let result = parse_markdown(input.as_bytes(), 2).unwrap();
        for section in &result.sections {
            let slice = &input[section.byte_start..section.byte_end];
            if section.heading_level > 0 {
                // Section should start with the heading
                assert!(
                    slice.starts_with('#'),
                    "section at {}..{} = {:?}",
                    section.byte_start,
                    section.byte_end,
                    slice
                );
            }
        }
    }
}
