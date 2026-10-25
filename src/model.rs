//! Markdown -> Word 的中间文档模型。
//!
//! 分成两层：`Block`（块级：标题/段落/代码块/列表/表格/图片/公式）与
//! `Inline`（行级：文本/强调/代码/链接/行内图片/行内公式）。
//! 解析器只负责产出这个模型，写出器只负责消费它，两边互不干扰。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub enum Inline {
    Text(String),
    Code(String),
    Strong(Vec<Inline>),
    Emphasis(Vec<Inline>),
    Strikethrough(Vec<Inline>),
    Superscript(Vec<Inline>),
    Subscript(Vec<Inline>),
    Link { text: Vec<Inline>, url: String },
    Image { src: String, alt: String },
    /// 行内公式（`$...$`），保存原始 LaTeX
    Math(String),
    SoftBreak,
    HardBreak,
    FootnoteRef(String),
}

#[derive(Debug, Clone)]
pub struct ListItem {
    pub content: Vec<Block>,
    /// `Some(true/false)` 表示 `- [x]` / `- [ ]` 任务列表
    pub task: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum Block {
    Heading { level: u8, text: Vec<Inline> },
    Paragraph(Vec<Inline>),
    CodeBlock { lang: String, code: String },
    Quote(Vec<Block>),
    List { ordered: bool, start: u64, items: Vec<ListItem> },
    Table {
        aligns: Vec<Align>,
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// 独占一段的图片（`![alt](src)` 单独成段时）
    ImageBlock { src: String, alt: String, title: String },
    /// 块级公式（`$$...$$`）
    MathBlock(String),
    ThematicBreak,
    /// 无法解析的原始 HTML 块，以等宽文本降级呈现
    RawHtml(String),
}

#[derive(Debug, Clone, Default)]
pub struct Doc {
    pub blocks: Vec<Block>,
    /// 一级标题内容（取自首个 H1，用作文档标题）
    pub title: Option<String>,
}

/// 把一段行内内容压成纯文本（用于图片 alt、标题文本等）
pub fn inline_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Text(t) => s.push_str(t),
            Inline::Code(t) => s.push_str(t),
            Inline::Strong(v) | Inline::Emphasis(v) | Inline::Strikethrough(v)
            | Inline::Superscript(v) | Inline::Subscript(v) | Inline::Link { text: v, .. } => {
                s.push_str(&inline_text(v))
            }
            Inline::Image { alt, .. } => s.push_str(alt),
            Inline::Math(m) => s.push_str(m),
            Inline::SoftBreak | Inline::HardBreak => s.push(' '),
            Inline::FootnoteRef(n) => s.push_str(n),
        }
    }
    s
}

/// 判断字符是否属于 CJK / 全角标点。
/// 用于决定换行处要不要补空格：中英文混排时，中文内部换行不该产生空格。
pub fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{1100}'..='\u{11FF}'   // 韩文
        | '\u{2E80}'..='\u{A4CF}' // CJK 部首 / 汉字 / 日文
        | '\u{A960}'..='\u{A97F}'
        | '\u{AC00}'..='\u{D7A3}' // 韩文音节
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FE30}'..='\u{FE4F}' // CJK 兼容形式
        | '\u{FF00}'..='\u{FF60}' // 全角
        | '\u{FFE0}'..='\u{FFE6}'
        | '\u{20000}'..='\u{3FFFF}'
    )
}
