//! A small Markdown renderer for the release notes in the update window.
//!
//! It covers what the notes use: headings, bullet and numbered lists, bold,
//! italic, inline code, code blocks, quotes and rules. Links show their text
//! only, because the app never opens a browser. Anything it does not know is
//! shown as plain text.
//!
//! The notes come from the network, so the text is sanitized first: the
//! interface font cannot draw every character and egui refuses to lay out a
//! missing glyph.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, Stroke, TextFormat, Vec2};

use crate::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, String),
    Bullet(String),
    Numbered(String, String),
    Quote(String),
    Code(String),
    Paragraph(String),
    Rule,
    Space,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

/// Replace what the interface font cannot draw.
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\n' | '\t' => out.push(character),
            '→' | '⇒' | '›' => out.push_str("->"),
            '←' => out.push_str("<-"),
            '•' | '▪' | '◦' => out.push('·'),
            '✔' | '✅' | '✓' => out.push_str("OK"),
            c if c.is_ascii() && (c.is_ascii_graphic() || c == ' ') => out.push(c),
            // Latin-1 and Latin Extended: accents, ×, ·, …
            c if ('\u{00A0}'..='\u{024F}').contains(&c) => out.push(c),
            '’' | '‘' | '“' | '”' | '…' | '–' | '—' => out.push(character),
            c if c.is_whitespace() => out.push(' '),
            _ => out.push('?'),
        }
    }
    out
}

pub fn parse_blocks(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut fence: Option<String> = None;
    for raw in source.lines() {
        let line = raw.trim_end();
        if let Some(code) = fence.as_mut() {
            if line.trim_start().starts_with("```") {
                blocks.push(Block::Code(std::mem::take(code)));
                fence = None;
            } else {
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(line);
            }
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            fence = Some(String::new());
        } else if trimmed.is_empty() {
            if !matches!(blocks.last(), Some(Block::Space) | None) {
                blocks.push(Block::Space);
            }
        } else if let Some(level) = heading_level(trimmed) {
            let text = trimmed[level as usize..].trim().trim_matches('#').trim();
            blocks.push(Block::Heading(level, text.to_string()));
        } else if is_rule(trimmed) {
            blocks.push(Block::Rule);
        } else if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            blocks.push(Block::Bullet(rest.trim().to_string()));
        } else if let Some((number, rest)) = numbered(trimmed) {
            blocks.push(Block::Numbered(number, rest.trim().to_string()));
        } else if let Some(rest) = trimmed.strip_prefix('>') {
            blocks.push(Block::Quote(rest.trim().to_string()));
        } else if let Some(Block::Paragraph(text)) = blocks.last_mut() {
            text.push(' ');
            text.push_str(trimmed);
        } else {
            blocks.push(Block::Paragraph(trimmed.to_string()));
        }
    }
    if let Some(code) = fence {
        blocks.push(Block::Code(code));
    }
    while matches!(blocks.last(), Some(Block::Space)) {
        blocks.pop();
    }
    blocks
}

fn heading_level(line: &str) -> Option<u8> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    ((1..=6).contains(&hashes) && line[hashes..].starts_with(' ')).then_some(hashes as u8)
}

fn is_rule(line: &str) -> bool {
    line.len() >= 3 && line.chars().all(|c| c == '-' || c == '*' || c == '_')
}

fn numbered(line: &str) -> Option<(String, &str)> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = line[digits..].strip_prefix(". ")?;
    Some((format!("{}.", &line[..digits]), rest))
}

/// Split a line into styled spans: `**bold**`, `*italic*` or `_italic_`,
/// `` `code` ``, and `[text](url)` shown as its text.
pub fn parse_inline(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let (mut bold, mut italic) = (false, false);
    let mut index = 0;

    fn flush(spans: &mut Vec<Span>, plain: &mut String, bold: bool, italic: bool) {
        if !plain.is_empty() {
            spans.push(Span {
                text: std::mem::take(plain),
                bold,
                italic,
                code: false,
            });
        }
    }

    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();
        if c == '`' {
            if let Some(end) = chars[index + 1..].iter().position(|x| *x == '`') {
                flush(&mut spans, &mut plain, bold, italic);
                spans.push(Span {
                    text: chars[index + 1..index + 1 + end].iter().collect(),
                    bold,
                    italic,
                    code: true,
                });
                index += end + 2;
                continue;
            }
        } else if c == '*' && next == Some('*') {
            flush(&mut spans, &mut plain, bold, italic);
            bold = !bold;
            index += 2;
            continue;
        } else if (c == '*' || c == '_') && opens_or_closes_emphasis(&chars, index, italic) {
            flush(&mut spans, &mut plain, bold, italic);
            italic = !italic;
            index += 1;
            continue;
        } else if c == '[' {
            if let Some((label, after)) = link(&chars, index) {
                plain.push_str(&label);
                index = after;
                continue;
            }
        } else if c == '\\' && next.is_some_and(|n| "*_`[]()#\\".contains(n)) {
            plain.push(next.unwrap_or('\\'));
            index += 2;
            continue;
        }
        plain.push(c);
        index += 1;
    }
    flush(&mut spans, &mut plain, bold, italic);
    spans
}

/// A lone `*` or `_` is emphasis only when it hugs a word: `*this*`, not `2 * 3`
/// or `snake_case`.
fn opens_or_closes_emphasis(chars: &[char], index: usize, open: bool) -> bool {
    let before = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
    let after = chars.get(index + 1).copied();
    if open {
        return before.is_some_and(|c| !c.is_whitespace())
            && !after.is_some_and(char::is_alphanumeric);
    }
    let closes_later = chars[index + 1..].iter().any(|c| *c == chars[index]);
    after.is_some_and(|c| !c.is_whitespace())
        && !before.is_some_and(char::is_alphanumeric)
        && closes_later
}

fn link(chars: &[char], start: usize) -> Option<(String, usize)> {
    let close = chars[start + 1..].iter().position(|c| *c == ']')? + start + 1;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = chars[close + 2..].iter().position(|c| *c == ')')? + close + 2;
    Some((chars[start + 1..close].iter().collect(), end + 1))
}

/// The width is left to the label, which wraps at the room it really has.
fn job_for(spans: &[Span], base: &FontId, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    for span in spans {
        let format = TextFormat {
            font_id: if span.code {
                theme::mono(base.size - 1.0)
            } else {
                base.clone()
            },
            color: if span.code {
                theme::MINT
            } else if span.bold {
                theme::TEXT
            } else {
                color
            },
            italics: span.italic,
            background: if span.code {
                theme::BG_DEEP
            } else {
                Color32::TRANSPARENT
            },
            ..TextFormat::default()
        };
        job.append(&span.text, 0.0, format);
    }
    job
}

pub fn show(ui: &mut egui::Ui, source: &str) {
    let source = sanitize(source);
    let body = theme::proportional(11.5);
    for block in parse_blocks(&source) {
        match block {
            Block::Space => {
                ui.add_space(4.0);
            }
            Block::Rule => {
                ui.separator();
            }
            Block::Heading(level, text) => {
                let size = match level {
                    1 => 18.0,
                    2 => 15.0,
                    _ => 13.0,
                };
                ui.add_space(if level == 1 { 0.0 } else { 4.0 });
                ui.label(
                    egui::RichText::new(text)
                        .size(size)
                        .strong()
                        .color(theme::TEXT),
                );
            }
            Block::Paragraph(text) => {
                ui.add(egui::Label::new(job_for(&parse_inline(&text), &body, theme::MUTED)).wrap());
            }
            Block::Quote(text) => {
                ui.horizontal_top(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(3.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 1.0, theme::LINE);
                    ui.add(
                        egui::Label::new(job_for(&parse_inline(&text), &body, theme::LAVENDER))
                            .wrap(),
                    );
                });
            }
            Block::Code(text) => {
                egui::Frame::NONE
                    .fill(theme::BG_DEEP)
                    .stroke(Stroke::new(1.0, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(text)
                                    .font(theme::mono(10.0))
                                    .color(theme::MINT),
                            )
                            .wrap(),
                        );
                    });
            }
            Block::Bullet(text) => list_item(ui, None, &text, &body),
            Block::Numbered(number, text) => list_item(ui, Some(&number), &text, &body),
        }
    }
}

/// A list item with a hanging indent: the marker stays put, the text wraps.
fn list_item(ui: &mut egui::Ui, number: Option<&str>, text: &str, body: &FontId) {
    ui.horizontal_top(|ui| {
        ui.add_space(6.0);
        match number {
            Some(number) => {
                ui.label(
                    egui::RichText::new(number)
                        .font(body.clone())
                        .color(theme::LAVENDER),
                );
            }
            None => {
                // Drawn, not typed: the interface font has no bullet glyph.
                let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 16.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(rect.center() + Vec2::new(0.0, 1.0), 2.2, theme::CORAL);
            }
        }
        ui.add(egui::Label::new(job_for(&parse_inline(text), body, theme::MUTED)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> String {
        parse_inline(text)
            .iter()
            .map(|span| span.text.as_str())
            .collect()
    }

    #[test]
    fn blocks_follow_the_release_notes_layout() {
        let blocks = parse_blocks(
            "# Title\n\nIntro line\ncontinues.\n\n## New\n\n- one\n- two\n\n1. first\n2. second",
        );
        assert_eq!(
            blocks,
            vec![
                Block::Heading(1, "Title".into()),
                Block::Space,
                Block::Paragraph("Intro line continues.".into()),
                Block::Space,
                Block::Heading(2, "New".into()),
                Block::Space,
                Block::Bullet("one".into()),
                Block::Bullet("two".into()),
                Block::Space,
                Block::Numbered("1.".into(), "first".into()),
                Block::Numbered("2.".into(), "second".into()),
            ]
        );
    }

    #[test]
    fn code_fences_quotes_and_rules() {
        let blocks = parse_blocks("```bash\ncargo test\nls\n```\n> note\n---");
        assert_eq!(
            blocks,
            vec![
                Block::Code("cargo test\nls".into()),
                Block::Quote("note".into()),
                Block::Rule
            ]
        );
        // An unterminated fence still shows its content.
        assert_eq!(parse_blocks("```\nopen"), vec![Block::Code("open".into())]);
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        assert_eq!(
            parse_blocks("#hashtag"),
            vec![Block::Paragraph("#hashtag".into())]
        );
    }

    #[test]
    fn inline_styles_are_parsed() {
        let spans = parse_inline("a **bold** and *it* and `code` end");
        let kinds: Vec<(&str, bool, bool, bool)> = spans
            .iter()
            .map(|s| (s.text.as_str(), s.bold, s.italic, s.code))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("a ", false, false, false),
                ("bold", true, false, false),
                (" and ", false, false, false),
                ("it", false, true, false),
                (" and ", false, false, false),
                ("code", false, false, true),
                (" end", false, false, false),
            ]
        );
    }

    #[test]
    fn markers_that_are_not_styling_stay_as_text() {
        assert_eq!(plain("2 * 3 = 6"), "2 * 3 = 6");
        assert_eq!(plain("snake_case_name"), "snake_case_name");
        assert_eq!(plain("a lone ** marker"), "a lone  marker");
        assert_eq!(plain("escaped \\*star\\*"), "escaped *star*");
        assert_eq!(plain("open `tick"), "open `tick");
    }

    #[test]
    fn links_show_only_their_text() {
        assert_eq!(
            plain("see [the docs](https://example.com/x) now"),
            "see the docs now"
        );
        assert_eq!(plain("a [broken link"), "a [broken link");
    }

    #[test]
    fn the_real_notes_lose_every_marker() {
        let notes = include_str!("../../../docs/releases/v2.6.0.md");
        for block in parse_blocks(notes) {
            let text = match block {
                Block::Heading(_, t) | Block::Bullet(t) | Block::Paragraph(t) | Block::Quote(t) => {
                    t
                }
                Block::Numbered(_, t) => t,
                _ => continue,
            };
            let shown = plain(&text);
            assert!(!shown.contains("**"), "{shown}");
            assert!(!shown.contains('`'), "{shown}");
        }
    }

    #[test]
    fn sanitizing_keeps_what_the_font_draws_and_replaces_the_rest() {
        assert_eq!(sanitize("Settings → Updates"), "Settings -> Updates");
        assert_eq!(sanitize("è ’quote’ … – ×"), "è ’quote’ … – ×");
        assert_eq!(sanitize("done ✔ ✓"), "done OK OK");
        assert_eq!(sanitize("日本"), "??");
        assert_eq!(sanitize("a\u{200b}b"), "a?b");
        assert_eq!(sanitize("line\nbreak"), "line\nbreak");
    }
}
