//! GFM Markdown parser matching `VisionAnswerMarkdown.tsx` & `vision-answer-markdown.css`.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone)]
pub struct MdSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    #[allow(dead_code)]
    pub strikethrough: bool,
}

#[derive(Debug, Clone)]
pub enum MdBlock {
    Heading {
        level: u8,
        spans: Vec<MdSpan>,
    },
    Paragraph(Vec<MdSpan>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<MdListItem>,
    },
    Blockquote(Vec<MdBlock>),
    CodeBlock {
        code: String,
    },
    Rule,
    Table {
        headers: Vec<Vec<MdSpan>>,
        rows: Vec<Vec<Vec<MdSpan>>>,
    },
}

#[derive(Debug, Clone)]
pub struct MdListItem {
    pub checked: Option<bool>,
    pub blocks: Vec<MdBlock>,
}

#[derive(Clone, Copy, Default)]
struct InlineState {
    bold: bool,
    italic: bool,
    strikethrough: bool,
}

pub fn parse_markdown(source: &str) -> Vec<MdBlock> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(source, options);

    enum Frame {
        Root(Vec<MdBlock>),
        Heading(u8, Vec<MdSpan>),
        Paragraph(Vec<MdSpan>),
        List {
            ordered: bool,
            start: u64,
            items: Vec<MdListItem>,
        },
        ListItem {
            checked: Option<bool>,
            blocks: Vec<MdBlock>,
            inline: Vec<MdSpan>,
        },
        Blockquote(Vec<MdBlock>),
        CodeBlock(String),
        Table {
            headers: Vec<Vec<MdSpan>>,
            rows: Vec<Vec<Vec<MdSpan>>>,
            in_head: bool,
            current_row: Vec<Vec<MdSpan>>,
            current_cell: Option<Vec<MdSpan>>,
        },
    }

    let mut stack = vec![Frame::Root(Vec::new())];
    let mut inline = InlineState::default();

    let push_span = |stack: &mut Vec<Frame>, span: MdSpan| {
        if span.text.is_empty() {
            return;
        }
        match stack.last_mut() {
            Some(Frame::Heading(_, spans) | Frame::Paragraph(spans)) => spans.push(span),
            Some(Frame::ListItem { inline: spans, .. }) => spans.push(span),
            Some(Frame::CodeBlock(code)) => code.push_str(&span.text),
            Some(Frame::Table {
                current_cell: Some(spans),
                ..
            }) => spans.push(span),
            _ => {}
        }
    };

    let push_block = |stack: &mut Vec<Frame>, block: MdBlock| match stack.last_mut() {
        Some(Frame::Root(blocks) | Frame::Blockquote(blocks)) => blocks.push(block),
        Some(Frame::ListItem {
            blocks,
            inline: spans,
            ..
        }) => {
            if !spans.is_empty() {
                blocks.push(MdBlock::Paragraph(std::mem::take(spans)));
            }
            blocks.push(block);
        }
        _ => {}
    };

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    let lvl = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    stack.push(Frame::Heading(lvl, Vec::new()));
                }
                Tag::Paragraph => stack.push(Frame::Paragraph(Vec::new())),
                Tag::List(start) => stack.push(Frame::List {
                    ordered: start.is_some(),
                    start: start.unwrap_or(1),
                    items: Vec::new(),
                }),
                Tag::Item => stack.push(Frame::ListItem {
                    checked: None,
                    blocks: Vec::new(),
                    inline: Vec::new(),
                }),
                Tag::BlockQuote(_) => stack.push(Frame::Blockquote(Vec::new())),
                Tag::CodeBlock(CodeBlockKind::Indented | CodeBlockKind::Fenced(_)) => {
                    stack.push(Frame::CodeBlock(String::new()));
                }
                Tag::Table(_) => stack.push(Frame::Table {
                    headers: Vec::new(),
                    rows: Vec::new(),
                    in_head: false,
                    current_row: Vec::new(),
                    current_cell: None,
                }),
                Tag::TableHead => {
                    if let Some(Frame::Table {
                        in_head,
                        current_row,
                        ..
                    }) = stack.last_mut()
                    {
                        *in_head = true;
                        current_row.clear();
                    }
                }
                Tag::TableRow => {
                    if let Some(Frame::Table { current_row, .. }) = stack.last_mut() {
                        current_row.clear();
                    }
                }
                Tag::TableCell => {
                    if let Some(Frame::Table { current_cell, .. }) = stack.last_mut() {
                        *current_cell = Some(Vec::new());
                    }
                }
                Tag::Strong => inline.bold = true,
                Tag::Emphasis => inline.italic = true,
                Tag::Strikethrough => inline.strikethrough = true,
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    if let Some(Frame::Heading(level, spans)) = stack.pop() {
                        push_block(&mut stack, MdBlock::Heading { level, spans });
                    }
                }
                TagEnd::Paragraph => {
                    if let Some(Frame::Paragraph(spans)) = stack.pop() {
                        push_block(&mut stack, MdBlock::Paragraph(spans));
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Frame::List {
                        ordered,
                        start,
                        items,
                    }) = stack.pop()
                    {
                        push_block(
                            &mut stack,
                            MdBlock::List {
                                ordered,
                                start,
                                items,
                            },
                        );
                    }
                }
                TagEnd::Item => {
                    if let Some(Frame::ListItem {
                        checked,
                        mut blocks,
                        inline: spans,
                    }) = stack.pop()
                    {
                        if !spans.is_empty() {
                            blocks.insert(0, MdBlock::Paragraph(spans));
                        }
                        if let Some(Frame::List { items, .. }) = stack.last_mut() {
                            items.push(MdListItem { checked, blocks });
                        }
                    }
                }
                TagEnd::BlockQuote(_) => {
                    if let Some(Frame::Blockquote(blocks)) = stack.pop() {
                        push_block(&mut stack, MdBlock::Blockquote(blocks));
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(Frame::CodeBlock(mut code)) = stack.pop() {
                        while code.ends_with('\n') {
                            code.pop();
                        }
                        push_block(&mut stack, MdBlock::CodeBlock { code });
                    }
                }
                TagEnd::Table => {
                    if let Some(Frame::Table { headers, rows, .. }) = stack.pop() {
                        push_block(&mut stack, MdBlock::Table { headers, rows });
                    }
                }
                TagEnd::TableHead => {
                    if let Some(Frame::Table {
                        headers,
                        in_head,
                        current_row,
                        ..
                    }) = stack.last_mut()
                    {
                        *headers = std::mem::take(current_row);
                        *in_head = false;
                    }
                }
                TagEnd::TableRow => {
                    if let Some(Frame::Table {
                        rows, current_row, ..
                    }) = stack.last_mut()
                    {
                        rows.push(std::mem::take(current_row));
                    }
                }
                TagEnd::TableCell => {
                    if let Some(Frame::Table {
                        current_row,
                        current_cell,
                        ..
                    }) = stack.last_mut()
                        && let Some(cell) = current_cell.take()
                    {
                        current_row.push(cell);
                    }
                }
                TagEnd::Strong => inline.bold = false,
                TagEnd::Emphasis => inline.italic = false,
                TagEnd::Strikethrough => inline.strikethrough = false,
                _ => {}
            },
            Event::Text(t) => push_span(
                &mut stack,
                MdSpan {
                    text: t.into_string(),
                    bold: inline.bold,
                    italic: inline.italic,
                    code: false,
                    strikethrough: inline.strikethrough,
                },
            ),
            Event::Code(t) => push_span(
                &mut stack,
                MdSpan {
                    text: t.into_string(),
                    bold: inline.bold,
                    italic: inline.italic,
                    code: true,
                    strikethrough: inline.strikethrough,
                },
            ),
            Event::SoftBreak => push_span(
                &mut stack,
                MdSpan {
                    text: " ".into(),
                    bold: inline.bold,
                    italic: inline.italic,
                    code: false,
                    strikethrough: inline.strikethrough,
                },
            ),
            Event::HardBreak => push_span(
                &mut stack,
                MdSpan {
                    text: "\n".into(),
                    bold: inline.bold,
                    italic: inline.italic,
                    code: false,
                    strikethrough: inline.strikethrough,
                },
            ),
            Event::Rule => push_block(&mut stack, MdBlock::Rule),
            Event::TaskListMarker(checked) => {
                if let Some(Frame::ListItem { checked: slot, .. }) = stack.last_mut() {
                    *slot = Some(checked);
                }
            }
            Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {}
        }
    }

    match stack.into_iter().next() {
        Some(Frame::Root(blocks)) => blocks,
        _ => Vec::new(),
    }
}
