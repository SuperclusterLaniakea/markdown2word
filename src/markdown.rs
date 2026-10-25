//! Markdown 解析：pulldown-cmark 事件流 -> [`crate::model::Doc`]
//!
//! 用一个显式的"帧栈"处理嵌套结构。每个帧要么是块容器（收集 `Vec<Block>`），
//! 要么是行内容器（收集 `Vec<Inline>`），弹出时把结果挂回父帧。

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::model::*;

#[derive(Debug, Clone, Copy)]
enum BKind {
    Root,
    Quote,
    Item { task: Option<bool> },
    Discard,
}

#[derive(Debug, Clone)]
enum IKind {
    Heading(u8),
    Para,
    Strong,
    Em,
    Strike,
    Sup,
    Sub,
    Link(String),
    Cell,
    Image { src: String },
}

#[derive(Debug)]
enum Frame {
    Blocks { blocks: Vec<Block>, kind: BKind },
    Inline { kind: IKind, buf: Vec<Inline> },
    List { ordered: bool, start: u64, items: Vec<ListItem> },
    Table {
        aligns: Vec<Align>,
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        row: Vec<Vec<Inline>>,
        in_head: bool,
    },
    Code { lang: String, code: String },
    Html(String),
}

struct Builder {
    stack: Vec<Frame>,
    pending_break: bool,
    last_char: Option<char>,
    /// pulldown-cmark 会把 `$$...$$` 包在 Paragraph 里发出来，
    /// 我们把它提升成独立的块级公式后，要跳过紧随的 End(Paragraph)
    skip_para_end: bool,
}

impl Builder {
    fn new() -> Self {
        Self {
            stack: vec![Frame::Blocks { blocks: vec![], kind: BKind::Root }],
            pending_break: false,
            last_char: None,
            skip_para_end: false,
        }
    }

    /// `$$...$$` 独占一段时提升为块级公式；与文字混排时仍按行内处理
    fn display_math(&mut self, latex: String) {
        let is_own_para = matches!(self.stack.last(),
            Some(Frame::Inline { kind: IKind::Para, buf }) if buf.is_empty());
        if is_own_para {
            self.stack.pop();
            self.push_block(Block::MathBlock(latex));
            self.skip_para_end = true;
        } else {
            self.push_inline(Inline::Math(latex));
        }
    }

    fn push_block(&mut self, b: Block) {
        if let Some(Frame::Blocks { blocks, .. }) = self.stack.last_mut() {
            blocks.push(b);
        }
    }

    fn push_inline(&mut self, i: Inline) {
        if let Some(Frame::Inline { buf, .. }) = self.stack.last_mut() {
            buf.push(i);
        }
    }

    fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        // 换行补空格的策略：中文内部换行不补，英文单词之间补一个空格
        if self.pending_break {
            self.pending_break = false;
            let first = s.chars().next().unwrap_or(' ');
            let need_space = match self.last_char {
                Some(p) => !is_cjk(p) && !is_cjk(first),
                None => false,
            };
            if need_space {
                self.push_inline(Inline::Text(" ".into()));
            }
        }
        self.push_inline(Inline::Text(s.into()));
        self.last_char = s.chars().last();
    }

    fn finish(mut self) -> Doc {
        let mut blocks = match self.stack.pop() {
            Some(Frame::Blocks { blocks, .. }) => blocks,
            _ => vec![],
        };
        normalize(&mut blocks);
        let title = blocks.iter().find_map(|b| match b {
            Block::Heading { level: 1, text } => {
                let t = inline_text(text).trim().to_string();
                (!t.is_empty()).then_some(t)
            }
            _ => None,
        });
        Doc { blocks, title }
    }
}

/// 把"只有一张图片"的段落提升为独占段落的图片块，
/// 这样写出器才能把它居中、单独成段、并配题注。
fn normalize(blocks: &mut Vec<Block>) {
    for b in blocks.iter_mut() {
        match b {
            Block::Paragraph(inlines)
                if inlines.len() == 1 && matches!(inlines[0], Inline::Image { .. }) =>
            {
                if let Inline::Image { src, alt } = inlines.remove(0) {
                    *b = Block::ImageBlock { src, alt, title: String::new() };
                }
            }
            Block::Quote(inner) => normalize(inner),
            Block::List { items, .. } => {
                for it in items.iter_mut() {
                    normalize(&mut it.content);
                }
            }
            _ => {}
        }
    }
}

fn hlevel(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn conv_align(a: &Alignment) -> Align {
    match a {
        Alignment::None => Align::None,
        Alignment::Left => Align::Left,
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
    }
}

pub fn parse(md: &str) -> Doc {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_MATH);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_SUPERSCRIPT);
    opts.insert(Options::ENABLE_SUBSCRIPT);
    opts.insert(Options::ENABLE_DEFINITION_LIST);

    let mut b = Builder::new();

    for ev in Parser::new_ext(md, opts) {
        match ev {
            Event::Start(tag) => b.start(tag),
            Event::End(end) => b.end(end),
            Event::Text(t) => b.text(&t),
            Event::Code(c) => {
                b.pending_break = false;
                b.push_inline(Inline::Code(c.into_string()));
            }
            Event::Html(h) | Event::InlineHtml(h) => b.html(&h),
            Event::SoftBreak => b.pending_break = true,
            Event::HardBreak => {
                b.pending_break = false;
                b.push_inline(Inline::HardBreak);
            }
            Event::Rule => b.push_block(Block::ThematicBreak),
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Blocks { kind: BKind::Item { task }, .. }) = self_stack_last(&mut b.stack)
                {
                    *task = Some(checked);
                }
            }
            Event::FootnoteReference(name) => {
                b.push_inline(Inline::FootnoteRef(name.into_string()))
            }
            Event::InlineMath(m) => b.push_inline(Inline::Math(m.into_string())),
            Event::DisplayMath(m) => b.display_math(m.into_string()),
        }
    }

    b.finish()
}

fn self_stack_last(stack: &mut [Frame]) -> Option<&mut Frame> {
    stack.last_mut()
}

impl Builder {
    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.stack.push(Frame::Inline { kind: IKind::Para, buf: vec![] }),
            Tag::Heading { level, .. } => self.stack.push(Frame::Inline {
                kind: IKind::Heading(hlevel(level)),
                buf: vec![],
            }),
            Tag::BlockQuote(_) => {
                self.stack.push(Frame::Blocks { blocks: vec![], kind: BKind::Quote })
            }
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.into_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.stack.push(Frame::Code { lang, code: String::new() });
            }
            Tag::List(ol) => self.stack.push(Frame::List {
                ordered: ol.is_some(),
                start: ol.unwrap_or(1),
                items: vec![],
            }),
            Tag::Item => self
                .stack
                .push(Frame::Blocks { blocks: vec![], kind: BKind::Item { task: None } }),
            Tag::Emphasis => self.stack.push(Frame::Inline { kind: IKind::Em, buf: vec![] }),
            Tag::Strong => self.stack.push(Frame::Inline { kind: IKind::Strong, buf: vec![] }),
            Tag::Strikethrough => {
                self.stack.push(Frame::Inline { kind: IKind::Strike, buf: vec![] })
            }
            Tag::Superscript => self.stack.push(Frame::Inline { kind: IKind::Sup, buf: vec![] }),
            Tag::Subscript => self.stack.push(Frame::Inline { kind: IKind::Sub, buf: vec![] }),
            Tag::Link { dest_url, .. } => self.stack.push(Frame::Inline {
                kind: IKind::Link(dest_url.into_string()),
                buf: vec![],
            }),
            Tag::Image { dest_url, .. } => self.stack.push(Frame::Inline {
                kind: IKind::Image { src: dest_url.into_string() },
                buf: vec![],
            }),
            Tag::Table(aligns) => self.stack.push(Frame::Table {
                aligns: aligns.iter().map(conv_align).collect(),
                header: vec![],
                rows: vec![],
                row: vec![],
                in_head: false,
            }),
            Tag::TableHead => {
                if let Some(Frame::Table { in_head, .. }) = self.stack.last_mut() {
                    *in_head = true;
                }
            }
            Tag::TableRow => {
                if let Some(Frame::Table { row, .. }) = self.stack.last_mut() {
                    row.clear();
                }
            }
            Tag::TableCell => self.stack.push(Frame::Inline { kind: IKind::Cell, buf: vec![] }),
            Tag::HtmlBlock => self.stack.push(Frame::Html(String::new())),
            Tag::FootnoteDefinition(_) | Tag::MetadataBlock(_) => {
                self.stack.push(Frame::Blocks { blocks: vec![], kind: BKind::Discard })
            }
            Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => {
                self.stack.push(Frame::Blocks { blocks: vec![], kind: BKind::Discard })
            }
            _ => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph => {
                if self.skip_para_end {
                    self.skip_para_end = false;
                    return;
                }
                if let Some(Frame::Inline { kind: IKind::Para, buf }) = self.stack.pop() {
                    self.push_block(Block::Paragraph(buf));
                }
            }
            TagEnd::Heading(_) => {
                if let Some(Frame::Inline { kind: IKind::Heading(level), buf }) = self.stack.pop()
                {
                    self.push_block(Block::Heading { level, text: buf });
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(Frame::Blocks { blocks, kind: BKind::Quote }) = self.stack.pop() {
                    self.push_block(Block::Quote(blocks));
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Frame::Code { lang, code }) = self.stack.pop() {
                    self.push_block(Block::CodeBlock { lang, code });
                }
            }
            TagEnd::Item => {
                if let Some(Frame::Blocks { blocks, kind: BKind::Item { task } }) = self.stack.pop()
                {
                    if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                        items.push(ListItem { content: blocks, task });
                    }
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List { ordered, start, items }) = self.stack.pop() {
                    self.push_block(Block::List { ordered, start, items });
                }
            }
            TagEnd::Emphasis => {
                if let Some(Frame::Inline { kind: IKind::Em, buf }) = self.stack.pop() {
                    self.push_inline(Inline::Emphasis(buf));
                }
            }
            TagEnd::Strong => {
                if let Some(Frame::Inline { kind: IKind::Strong, buf }) = self.stack.pop() {
                    self.push_inline(Inline::Strong(buf));
                }
            }
            TagEnd::Strikethrough => {
                if let Some(Frame::Inline { kind: IKind::Strike, buf }) = self.stack.pop() {
                    self.push_inline(Inline::Strikethrough(buf));
                }
            }
            TagEnd::Superscript => {
                if let Some(Frame::Inline { kind: IKind::Sup, buf }) = self.stack.pop() {
                    self.push_inline(Inline::Superscript(buf));
                }
            }
            TagEnd::Subscript => {
                if let Some(Frame::Inline { kind: IKind::Sub, buf }) = self.stack.pop() {
                    self.push_inline(Inline::Subscript(buf));
                }
            }
            TagEnd::Link => {
                if let Some(Frame::Inline { kind: IKind::Link(url), buf }) = self.stack.pop() {
                    let text = if buf.is_empty() { vec![Inline::Text(url.clone())] } else { buf };
                    self.push_inline(Inline::Link { text, url });
                }
            }
            TagEnd::Image => {
                if let Some(Frame::Inline { kind: IKind::Image { src }, buf }) = self.stack.pop() {
                    self.push_inline(Inline::Image { src, alt: inline_text(&buf) });
                }
            }
            TagEnd::TableCell => {
                if let Some(Frame::Inline { kind: IKind::Cell, buf }) = self.stack.pop() {
                    if let Some(Frame::Table { row, .. }) = self.stack.last_mut() {
                        row.push(buf);
                    }
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::Table { header, rows, row, in_head, .. }) = self.stack.last_mut()
                {
                    let r = std::mem::take(row);
                    if *in_head {
                        *header = r;
                    } else {
                        rows.push(r);
                    }
                }
            }
            TagEnd::TableHead => {
                if let Some(Frame::Table { in_head, .. }) = self.stack.last_mut() {
                    *in_head = false;
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table { aligns, header, rows, .. }) = self.stack.pop() {
                    self.push_block(Block::Table { aligns, header, rows });
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(Frame::Html(s)) = self.stack.pop() {
                    if !s.trim().is_empty() {
                        self.push_block(Block::RawHtml(s));
                    }
                }
            }
            TagEnd::FootnoteDefinition | TagEnd::MetadataBlock(_) => {
                let _ = self.stack.pop();
            }
            TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => {
                let _ = self.stack.pop();
            }
            _ => {}
        }
    }

    fn html(&mut self, h: &str) {
        // 块级 HTML 直接收集，行内 HTML 只处理 <img> / <br>，其余忽略
        if let Some(Frame::Html(buf)) = self.stack.last_mut() {
            buf.push_str(h);
            return;
        }
        let low = h.to_ascii_lowercase();
        if low.contains("<img") {
            if let Some(src) = extract_attr(h, "src") {
                let alt = extract_attr(h, "alt").unwrap_or_default();
                self.push_inline(Inline::Image { src, alt });
            }
        } else if low.contains("<br") {
            self.push_inline(Inline::HardBreak);
        }
    }
}

fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let pat = name.as_bytes();
    let mut i = 0;
    while i + pat.len() < bytes.len() {
        if &bytes[i..i + pat.len()] == pat {
            let mut j = i + pat.len();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'=' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                    let q = bytes[j];
                    j += 1;
                    let start = j;
                    while j < bytes.len() && bytes[j] != q {
                        j += 1;
                    }
                    return Some(tag[start..j].to_string());
                }
            }
        }
        i += 1;
    }
    None
}
