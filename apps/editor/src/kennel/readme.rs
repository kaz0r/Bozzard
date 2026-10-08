//! Enough Markdown for package READMEs: headings, paragraphs, bullets, tables and fenced code,
//! with inline code, bold and links. Links show their text only; nothing is fetched.
use crate::theme;
use eframe::egui::{self, RichText, TextFormat, text::LayoutJob};

#[derive(Debug, PartialEq)]
enum Block {
    Heading(usize, String),
    Paragraph(String),
    Bullet(String),
    Code(String),
    Table(Vec<Vec<String>>),
}

pub fn show(ui: &mut egui::Ui, text: &str) {
    for block in blocks(text) {
        match block {
            Block::Heading(level, text) => {
                ui.add_space(if level <= 2 { 6.0 } else { 3.0 });
                let size = match level {
                    1 => 18.0,
                    2 => 15.0,
                    _ => 13.0,
                };
                ui.label(RichText::new(text).font(theme::bold(ui.ctx(), size)));
            }
            Block::Paragraph(text) => {
                ui.add(egui::Label::new(inline(ui, &text)).wrap());
                ui.add_space(4.0);
            }
            Block::Bullet(text) => {
                ui.horizontal_top(|ui| {
                    ui.label("•");
                    ui.add(egui::Label::new(inline(ui, &text)).wrap());
                });
            }
            Block::Code(code) => {
                egui::Frame::new()
                    .fill(theme::glass(8))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(code).monospace()).wrap());
                    });
                ui.add_space(4.0);
            }
            Block::Table(rows) => {
                // One wrapped line per row; a grid can't narrow its columns to the pane.
                for row in rows {
                    ui.horizontal_wrapped(|ui| {
                        for (column, cell) in row.iter().enumerate() {
                            let mut text = inline(ui, cell);
                            if column == 0 {
                                for section in &mut text.sections {
                                    section.format.color = ui.visuals().weak_text_color();
                                }
                            }
                            ui.add(egui::Label::new(text).wrap());
                        }
                    });
                }
                ui.add_space(4.0);
            }
        }
    }
}

fn blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            let mut code = String::new();
            for line in lines.by_ref() {
                if line.trim_start().starts_with("```") {
                    break;
                }
                code.push_str(line);
                code.push('\n');
            }
            blocks.push(Block::Code(code.trim_end().to_owned()));
        } else if trimmed.is_empty() {
            continue;
        } else if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            blocks.push(Block::Heading(level, trimmed[level..].trim().to_owned()));
        } else if trimmed.starts_with('|') {
            let mut rows = vec![cells(trimmed)];
            while let Some(next) = lines.peek().copied().filter(|l| l.trim().starts_with('|')) {
                rows.push(cells(next));
                lines.next();
            }
            // Drop the alignment row and an empty header row.
            rows.retain(|row| {
                !row.iter()
                    .all(|cell| cell.chars().all(|c| matches!(c, '-' | ':')))
            });
            if !rows.is_empty() {
                blocks.push(Block::Table(rows));
            }
        } else if let Some(item) = bullet(trimmed) {
            let mut item = item.to_owned();
            while let Some(next) = lines
                .peek()
                .copied()
                .filter(|l| l.starts_with(' ') && !starts_block(l.trim()))
            {
                item.push(' ');
                item.push_str(next.trim());
                lines.next();
            }
            blocks.push(Block::Bullet(item));
        } else {
            let mut paragraph = trimmed.to_owned();
            while let Some(next) = lines.peek().copied().filter(|l| !starts_block(l.trim())) {
                paragraph.push(' ');
                paragraph.push_str(next.trim());
                lines.next();
            }
            blocks.push(Block::Paragraph(paragraph));
        }
    }
    blocks
}

fn bullet(line: &str) -> Option<&str> {
    line.strip_prefix("- ").or_else(|| line.strip_prefix("* "))
}

fn starts_block(line: &str) -> bool {
    line.is_empty()
        || line.starts_with('#')
        || line.starts_with('|')
        || line.starts_with("```")
        || bullet(line).is_some()
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// Body text with `code`, **bold** and [link](url) spans.
fn inline(ui: &egui::Ui, text: &str) -> LayoutJob {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let bold = theme::bold(ui.ctx(), body.size);
    let color = ui.visuals().text_color();
    let mut job = LayoutJob::default();
    let mut strong = false;
    let mut rest = text;
    while !rest.is_empty() {
        let font = if strong { bold.clone() } else { body.clone() };
        if let Some(after) = rest.strip_prefix("**") {
            strong = !strong;
            rest = after;
        } else if let Some(after) = rest.strip_prefix('`')
            && let Some(end) = after.find('`')
        {
            let format = TextFormat {
                font_id: mono.clone(),
                color,
                background: theme::glass(16),
                ..Default::default()
            };
            job.append(&after[..end], 0.0, format);
            rest = &after[end + 1..];
        } else if let Some(after) = rest.strip_prefix('[')
            && let Some((label, tail)) = after.split_once("](")
            && !label.contains(']')
            && let Some(end) = tail.find(')')
        {
            job.append(label, 0.0, TextFormat::simple(font, theme::ACCENT));
            rest = &tail[end + 1..];
        } else {
            let end = rest
                .char_indices()
                .skip(1)
                .find(|(_, c)| matches!(c, '*' | '`' | '['))
                .map_or(rest.len(), |(i, _)| i);
            job.append(&rest[..end], 0.0, TextFormat::simple(font, color));
            rest = &rest[end..];
        }
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_blocks_join_wrapped_lines_and_drop_table_rules() {
        let text = "# Steam\n\nSteamworks for Bozzard\ngames.\n\n| | |\n| --- | --- |\n| \
                    Package | `steam` 1.0.0 |\n\n## Install\n\n```sh\nkennel install steam\n```\n\n\
                    - **480** is Valve's app.\n  It opens directly.\n- Second\n";
        assert_eq!(
            blocks(text),
            [
                Block::Heading(1, "Steam".into()),
                Block::Paragraph("Steamworks for Bozzard games.".into()),
                Block::Table(vec![vec!["Package".into(), "`steam` 1.0.0".into()]]),
                Block::Heading(2, "Install".into()),
                Block::Code("kennel install steam".into()),
                Block::Bullet("**480** is Valve's app. It opens directly.".into()),
                Block::Bullet("Second".into()),
            ]
        );
    }

    #[test]
    fn inline_spans_keep_text_and_never_split_characters() {
        let ctx = egui::Context::default();
        let mut text = String::new();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            text = inline(
                ui,
                "Use `sdk/` — see [NOTICE.md](NOTICE.md), **≥ 0.1** * x [",
            )
            .text;
        });
        output.textures_delta.clear();
        assert_eq!(text, "Use sdk/ — see NOTICE.md, ≥ 0.1 * x [");
    }
}
