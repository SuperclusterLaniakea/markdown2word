//! 中间文档模型 -> .docx
//!
//! 文档结构（段落/表格/图片/样式/编号）交给 docx-rs 生成，公式部分先放一个
//! 占位 run，生成完 `document.xml` 之后再把那个 run 整段替换成真正的 OMML
//! —— 这样既能用上 docx-rs 稳定的包结构，又不受它元素枚举的限制（比如矩阵）。

use std::path::{Path, PathBuf};

use docx_rs::*;

use crate::model::*;
use crate::omml;

// ---------------------------------------------------------------- 配置

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Options {
    /// 正文中文文字体
    pub body_font: String,
    /// 正文西文字体
    pub body_font_en: String,
    /// 代码等宽字体
    pub mono_font: String,
    /// 公式字体
    pub math_font: String,
    /// 正文字号（半磅，24 = 12pt = 小四）
    pub body_size: usize,
    /// H1..H6 字号（半磅）
    pub heading_sizes: [usize; 6],
    pub heading_color: String,
    /// 代码字号（半磅）
    pub code_size: usize,
    /// 引用文字颜色
    pub quote_color: String,
    /// 正文首行缩进 2 字符
    pub first_line_indent: bool,
    /// 正文行距（240 = 单倍，360 = 1.5 倍）
    pub line_spacing: i32,
    /// 表格显示边框
    pub table_borders: bool,
    /// 图片最大宽度（缇）
    pub image_max_twips: u32,
    /// 独占段落的图片居中
    pub center_images: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            body_font: "宋体".into(),
            body_font_en: "Times New Roman".into(),
            mono_font: "Consolas".into(),
            math_font: "Cambria Math".into(),
            body_size: 24, // 小四
            heading_sizes: [36, 32, 28, 26, 24, 24],
            heading_color: "1F3864".into(),
            code_size: 20,
            quote_color: "595959".into(),
            first_line_indent: true,
            line_spacing: 360,
            table_borders: true,
            image_max_twips: 9070,
            center_images: true,
        }
    }
}

const CONTENT_TWIPS: u32 = 9070; // A4(11906) - 左右各 1418 缇
const TOKEN_PREFIX: &str = "[MD2WORDMATH:";

// ---------------------------------------------------------------- 行内样式

#[derive(Clone, Default)]
struct Rs {
    size: usize,
    bold: bool,
    italic: bool,
    strike: bool,
    mono: bool,
    sup: bool,
    sub: bool,
    color: Option<String>,
    url: Option<String>,
    shading: Option<String>,
}

enum Seg {
    Text(String, Rs),
    Break,
    Image { src: String, alt: String },
    Math(String),
}

// ---------------------------------------------------------------- 写出器

struct W<'a> {
    opt: &'a Options,
    base: PathBuf,
    docx: Docx,
    /// (LaTeX, 是否块级)
    math: Vec<(String, bool)>,
}

#[derive(Clone, Copy, Default)]
struct Ctx {
    /// 列表嵌套层级
    level: usize,
    /// 是否处于引用块内
    quote: bool,
}

impl<'a> W<'a> {
    fn new(opt: &'a Options, md: &Path) -> Self {
        let base = md.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        W { opt, base, docx: Docx::new(), math: Vec::new() }
    }

    fn fonts(&self, mono: bool) -> RunFonts {
        let (a, ea) = if mono {
            (self.opt.mono_font.as_str(), self.opt.body_font.as_str())
        } else {
            (self.opt.body_font_en.as_str(), self.opt.body_font.as_str())
        };
        RunFonts::new().ascii(a).hi_ansi(a).east_asia(ea).cs(a)
    }

    fn style_run(&self, r: Run, rs: &Rs) -> Run {
        let mut r = r.fonts(self.fonts(rs.mono)).size(rs.size);
        if rs.bold {
            r = r.bold();
        }
        if rs.italic {
            r = r.italic();
        }
        if rs.strike {
            r = r.strike();
        }
        if rs.sup {
            r = r.superscript();
        }
        if rs.sub {
            r = r.subscript();
        }
        if let Some(c) = &rs.color {
            r = r.color(c.clone());
        }
        if let Some(s) = &rs.shading {
            r = r.shading(Shading::new().fill(s.clone()));
        }
        r
    }

    // ---- 行内扁平化

    fn flatten(&mut self, inlines: &[Inline], rs: Rs, out: &mut Vec<Seg>) {
        for i in inlines {
            match i {
                Inline::Text(t) => out.push(Seg::Text(t.clone(), rs.clone())),
                Inline::Code(t) => {
                    let mut r = rs.clone();
                    r.mono = true;
                    r.shading = Some("F2F2F2".into());
                    out.push(Seg::Text(t.clone(), r));
                }
                Inline::Strong(v) => {
                    let mut r = rs.clone();
                    r.bold = true;
                    self.flatten(v, r, out);
                }
                Inline::Emphasis(v) => {
                    let mut r = rs.clone();
                    r.italic = true;
                    self.flatten(v, r, out);
                }
                Inline::Strikethrough(v) => {
                    let mut r = rs.clone();
                    r.strike = true;
                    self.flatten(v, r, out);
                }
                Inline::Superscript(v) => {
                    let mut r = rs.clone();
                    r.sup = true;
                    self.flatten(v, r, out);
                }
                Inline::Subscript(v) => {
                    let mut r = rs.clone();
                    r.sub = true;
                    self.flatten(v, r, out);
                }
                Inline::Link { text, url } => {
                    let mut r = rs.clone();
                    r.url = Some(url.clone());
                    r.color = Some("0563C1".into());
                    let mut sub = Vec::new();
                    self.flatten(text, r, &mut sub);
                    if sub.is_empty() {
                        out.push(Seg::Text(url.clone(), rs.clone()));
                    } else {
                        out.extend(sub);
                    }
                }
                Inline::Image { src, alt } => out.push(Seg::Image { src: src.clone(), alt: alt.clone() }),
                Inline::Math(m) => out.push(Seg::Math(m.clone())),
                Inline::SoftBreak => {}
                Inline::HardBreak => out.push(Seg::Break),
                Inline::FootnoteRef(n) => {
                    let mut r = rs.clone();
                    r.sup = true;
                    out.push(Seg::Text(format!("[{n}]"), r));
                }
            }
        }
    }

    /// 把行内内容挂到一个段落上
    fn push_inlines(&mut self, mut p: Paragraph, inlines: &[Inline], rs: Rs) -> Paragraph {
        let mut segs = Vec::new();
        self.flatten(inlines, rs, &mut segs);
        for seg in segs {
            match seg {
                Seg::Text(t, rs) => {
                    let run = self.style_run(Run::new().add_text(t), &rs);
                    p = match &rs.url {
                        Some(url) => p.add_hyperlink(
                            Hyperlink::new(url.clone(), HyperlinkType::External).add_run(run),
                        ),
                        None => p.add_run(run),
                    };
                }
                Seg::Break => p = p.add_run(Run::new().add_break(BreakType::TextWrapping)),
                Seg::Image { src, alt } => {
                    if let Some(path) = self.resolve(&src) {
                        match self.pic(&path) {
                            Ok(pic) => p = p.add_run(Run::new().add_image(pic)),
                            Err(_) => p = self.missing_image(p, &src, &alt),
                        }
                    } else {
                        p = self.missing_image(p, &src, &alt);
                    }
                }
                Seg::Math(m) => {
                    let id = self.math.len();
                    self.math.push((m, false));
                    p = p.add_run(Run::new().add_text(format!("{TOKEN_PREFIX}{id}]")));
                }
            }
        }
        p
    }

    fn missing_image(&self, p: Paragraph, src: &str, alt: &str) -> Paragraph {
        let label = if alt.is_empty() { src } else { alt };
        p.add_run(
            Run::new()
                .add_text(format!("[图片缺失: {label}]"))
                .color("C00000")
                .italic()
                .size(self.opt.body_size),
        )
    }

    fn resolve(&self, src: &str) -> Option<PathBuf> {
        if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
            return None;
        }
        let clean = src.split('?').next().unwrap_or(src);
        let clean = percent_decode(clean);
        let p = PathBuf::from(&clean);
        if p.is_absolute() {
            return p.exists().then_some(p);
        }
        let cand = self.base.join(&p);
        cand.exists().then_some(cand)
    }

    fn pic(&self, path: &Path) -> anyhow::Result<Pic> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "ico" | "webp") {
            anyhow::bail!("不支持的图片格式: {ext}");
        }
        let buf = std::fs::read(path)?;
        let (wpx, hpx) = match imagesize::size(path) {
            Ok(s) => (s.width as u64, s.height as u64),
            Err(_) => (600u64, 400u64),
        };
        let mut w = (wpx as u64).saturating_mul(9525);
        let mut h = (hpx as u64).saturating_mul(9525);
        let max = self.opt.image_max_twips as u64 * 635;
        if w == 0 {
            w = max;
        }
        if w > max {
            h = h.saturating_mul(max) / w;
            w = max;
        }
        Ok(Pic::new(&buf).size(w as u32, h as u32))
    }

    // ---- 块级

    fn blocks(&mut self, blocks: &[Block], ctx: Ctx) {
        for b in blocks {
            self.block(b, ctx);
        }
    }

    fn block(&mut self, b: &Block, ctx: Ctx) {
        match b {
            Block::Heading { level, text } => {
                let l = (*level as usize).clamp(1, 6);
                let size = self.opt.heading_sizes[l - 1];
                let rs = Rs {
                    size: if ctx.quote { self.opt.body_size } else { size },
                    bold: true,
                    color: if ctx.quote { None } else { Some(self.opt.heading_color.clone()) },
                    ..Default::default()
                };
                let mut p = Paragraph::new()
                    .style(&format!("mdH{l}"))
                    .outline_lvl(l - 1)
                    .keep_next(true)
                    .line_spacing(LineSpacing::new().before(240).after(120));
                if ctx.quote {
                    p = p.indent(Some(480), None, None, None);
                }
                p = self.push_inlines(p, text, rs);
                self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
            }
            Block::Paragraph(inlines) => {
                let p = self.body_paragraph(inlines, ctx);
                self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
            }
            Block::CodeBlock { code, .. } => {
                self.docx = std::mem::take(&mut self.docx).add_table(self.code_table(code, ctx));
            }
            Block::Quote(inner) => {
                self.blocks(inner, Ctx { quote: true, ..ctx });
            }
            Block::List { ordered, items, .. } => self.list(*ordered, items, ctx.level),
            Block::Table { aligns, header, rows } => {
                self.docx = std::mem::take(&mut self.docx).add_table(self.table(aligns, header, rows));
            }
            Block::ImageBlock { src, alt, title } => self.image_block(src, alt, title),
            Block::MathBlock(latex) => {
                let id = self.math.len();
                self.math.push((latex.clone(), true));
                let p = Paragraph::new()
                    .align(AlignmentType::Center)
                    .line_spacing(LineSpacing::new().before(120).after(120))
                    .add_run(Run::new().add_text(format!("{TOKEN_PREFIX}{id}]")));
                self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
            }
            Block::ThematicBreak => {
                let p = Paragraph::new().set_border(
                    ParagraphBorder::new(ParagraphBorderPosition::Bottom)
                        .val(BorderType::Single)
                        .size(6)
                        .color("BFBFBF"),
                );
                self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
            }
            Block::RawHtml(html) => {
                let p = Paragraph::new().add_run(
                    Run::new()
                        .add_text(html.trim().to_string())
                        .fonts(self.fonts(true))
                        .size(self.opt.code_size)
                        .color("808080"),
                );
                self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
            }
        }
    }

    fn body_paragraph(&mut self, inlines: &[Inline], ctx: Ctx) -> Paragraph {
        let rs = Rs {
            size: self.opt.body_size,
            color: if ctx.quote { Some(self.opt.quote_color.clone()) } else { None },
            ..Default::default()
        };
        let mut p = Paragraph::new().style("mdBody");
        if ctx.quote {
            p = p
                .indent(Some(480), None, None, None)
                .set_border(
                    ParagraphBorder::new(ParagraphBorderPosition::Left)
                        .val(BorderType::Single)
                        .size(12)
                        .color("BFBFBF"),
                );
        } else if self.opt.first_line_indent {
            // 首行缩进 2 字符：1 字符 = 字号(半磅) × 10 缇
            p = p.indent(
                None,
                Some(SpecialIndentType::FirstLine(self.opt.body_size as i32 * 10 * 2)),
                None,
                None,
            );
        }
        if self.opt.line_spacing > 0 {
            p = p.line_spacing(
                LineSpacing::new().line(self.opt.line_spacing).line_rule(LineSpacingType::Auto),
            );
        }
        self.push_inlines(p, inlines, rs)
    }

    fn code_table(&mut self, code: &str, ctx: Ctx) -> Table {
        let rs = Rs { size: self.opt.code_size, mono: true, ..Default::default() };
        let mut p = Paragraph::new().line_spacing(LineSpacing::new().line(240));
        let lines: Vec<&str> = code.trim_end_matches('\n').split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                p = p.add_run(Run::new().add_break(BreakType::TextWrapping));
            }
            let text = if line.is_empty() { " " } else { *line };
            p = p.add_run(self.style_run(Run::new().add_text(text), &rs));
        }
        let width = (CONTENT_TWIPS - if ctx.quote { 480 } else { 0 }) as usize;
        let cell = TableCell::new()
            .add_paragraph(p)
            .width(width, WidthType::Dxa)
            .shading(Shading::new().fill("F5F5F5"))
            .clear_all_border();
        let mut t = Table::new(vec![TableRow::new(vec![cell])])
            .width(width, WidthType::Dxa)
            .clear_all_border();
        if ctx.quote {
            t = t.indent(480);
        }
        t
    }

    fn image_block(&mut self, src: &str, alt: &str, title: &str) {
        let mut p = Paragraph::new();
        p = if self.opt.center_images { p.align(AlignmentType::Center) } else { p };
        p = match self.resolve(src).and_then(|path| self.pic(&path).ok()) {
            Some(pic) => p.add_run(Run::new().add_image(pic)),
            None => self.missing_image(p, src, alt),
        };
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        let caption = if !title.is_empty() { title } else { "" };
        if !caption.is_empty() {
            let cp = Paragraph::new()
                .align(AlignmentType::Center)
                .add_run(
                    Run::new()
                        .add_text(format!("图：{caption}"))
                        .size(self.opt.body_size - 2)
                        .color("595959")
                        .italic(),
                );
            self.docx = std::mem::take(&mut self.docx).add_paragraph(cp);
        }
    }

    fn list(&mut self, ordered: bool, items: &[ListItem], level: usize) {
        let num_id = if ordered { 2 } else { 1 };
        let lv = level.min(4);
        for item in items {
            let mut first = true;
            if let Some(checked) = item.task {
                let mark = if checked { "☑ " } else { "☐ " };
                let mut p = Paragraph::new()
                    .numbering(NumberingId::new(num_id), IndentLevel::new(lv))
                    .add_run(
                        Run::new()
                            .add_text(mark)
                            .fonts(self.fonts(false))
                            .size(self.opt.body_size),
                    );
                if let Some(Block::Paragraph(inlines)) = item.content.first() {
                    p = self.push_inlines(p, inlines, Rs { size: self.opt.body_size, ..Default::default() });
                }
                self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
                first = false;
            }
            for b in &item.content {
                match b {
                    Block::Paragraph(inlines) if first => {
                        let mut p = Paragraph::new()
                            .numbering(NumberingId::new(num_id), IndentLevel::new(lv));
                        if self.opt.line_spacing > 0 {
                            p = p.line_spacing(
                                LineSpacing::new().line(self.opt.line_spacing).line_rule(LineSpacingType::Auto),
                            );
                        }
                        p = self.push_inlines(p, inlines, Rs { size: self.opt.body_size, ..Default::default() });
                        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
                        first = false;
                    }
                    Block::List { ordered: o2, items: sub, .. } => {
                        self.list(*o2, sub, lv + 1);
                    }
                    other => {
                        let mut p = match other {
                            Block::Paragraph(inlines) => {
                                let mut pp = Paragraph::new()
                                    .indent(Some(720 * (lv as i32 + 1)), None, None, None);
                                pp = self.push_inlines(
                                    pp,
                                    inlines,
                                    Rs { size: self.opt.body_size, ..Default::default() },
                                );
                                pp
                            }
                            _ => {
                                self.block(other, Ctx { level: lv + 1, quote: false });
                                continue;
                            }
                        };
                        self.docx = std::mem::take(&mut self.docx).add_paragraph(
                            std::mem::replace(&mut p, Paragraph::new()),
                        );
                    }
                }
            }
            if first {
                // 空列表项
                let p = Paragraph::new().numbering(NumberingId::new(num_id), IndentLevel::new(lv));
                self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
            }
        }
    }

    fn table(&mut self, aligns: &[Align], header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) -> Table {
        let ncols = header
            .len()
            .max(rows.first().map(|r| r.len()).unwrap_or(0))
            .max(1);
        let w = (CONTENT_TWIPS / ncols as u32).max(500) as usize;
        let mut trs: Vec<TableRow> = Vec::new();

        if !header.is_empty() {
            let mut cells = Vec::new();
            for (i, c) in header.iter().enumerate() {
                cells.push(self.cell(c, aligns.get(i).copied().unwrap_or(Align::None), true, w));
            }
            trs.push(TableRow::new(cells));
        }
        for r in rows {
            let mut cells = Vec::new();
            for (i, c) in r.iter().enumerate() {
                cells.push(self.cell(c, aligns.get(i).copied().unwrap_or(Align::None), false, w));
            }
            trs.push(TableRow::new(cells));
        }

        let mut t = Table::new(trs)
            .set_grid(vec![w; ncols])
            .width(w * ncols, WidthType::Dxa);
        if self.opt.table_borders {
            t = t.set_borders(TableBorders::default());
        } else {
            t = t.clear_all_border();
        }
        t
    }

    fn cell(&mut self, inlines: &[Inline], align: Align, head: bool, w: usize) -> TableCell {
        let mut p = Paragraph::new().line_spacing(LineSpacing::new().line(240));
        p = match align {
            Align::Center => p.align(AlignmentType::Center),
            Align::Right => p.align(AlignmentType::Right),
            _ => p,
        };
        let rs = Rs {
            size: self.opt.body_size,
            bold: head,
            ..Default::default()
        };
        p = self.push_inlines(p, inlines, rs);
        let mut c = TableCell::new().add_paragraph(p).width(w, WidthType::Dxa);
        if head {
            c = c.shading(Shading::new().fill("F2F2F2"));
        }
        c
    }
}

// ---------------------------------------------------------------- 入口

pub fn convert(md_path: &Path, out_path: &Path, opt: &Options) -> anyhow::Result<()> {
    convert_multi(&[md_path.to_path_buf()], out_path, opt)
}

/// 把多个 Markdown 合并成一个 docx（文件之间插入分页符）
pub fn convert_multi(paths: &[PathBuf], out_path: &Path, opt: &Options) -> anyhow::Result<()> {
    anyhow::ensure!(!paths.is_empty(), "没有输入文件");
    let mut w = W::new(opt, &paths[0]);
    for (i, p) in paths.iter().enumerate() {
        if i > 0 {
            let brk = Paragraph::new().page_break_before(true);
            w.docx = std::mem::take(&mut w.docx).add_paragraph(brk);
        }
        let text = std::fs::read_to_string(p)?;
        let doc = crate::markdown::parse(&text);
        // 图片相对路径以各自 Markdown 所在目录为基准
        w.base = p.parent().map(|x| x.to_path_buf()).unwrap_or_default();
        w.blocks(&doc.blocks, Ctx::default());
    }

    let mut docx = std::mem::take(&mut w.docx);
    // 用 docDefaults 设置全局默认字体/字号；
    // 注意不能再额外添加 styleId="Normal"（docx-rs 默认已经生成了一个，
    // 两个同名 styleId 会让 Word 判定文档损坏）
    let mut st = Styles::new()
        .default_fonts(default_fonts(opt))
        .default_size(opt.body_size);
    for s in styles(opt) {
        st = st.add_style(s);
    }
    docx = docx.styles(st);
    docx = docx
        .add_abstract_numbering(bullet_numbering(opt))
        .add_numbering(Numbering::new(1, 1))
        .add_abstract_numbering(decimal_numbering(opt))
        .add_numbering(Numbering::new(2, 2))
        .page_size(11906, 16838)
        .page_margin(PageMargin::new().top(1418).left(1418).bottom(1418).right(1418));

    let mut xml = docx.build();
    patch_math(&mut xml, &w.math, opt)?;
    // docx-rs 各 build_to 按字段顺序输出，与 ECMA-376 的 xsd:sequence 不一致
    // （w:pPr 把 w:rPr 放最前、w:rPr 把 w:sz 放最前……），Word 会判文档损坏。
    // 这里统一按规范顺序重排属性元素的子元素。
    for part in [
        &mut xml.document,
        &mut xml.styles,
        &mut xml.numberings,
        &mut xml.settings,
        &mut xml.font_table,
        &mut xml.comments,
        &mut xml.footnotes,
        &mut xml.comments_extended,
    ] {
        if let Ok(s) = std::str::from_utf8(part) {
            *part = crate::order::normalize(s).into_bytes();
        }
    }

    let file = std::fs::File::create(out_path)?;
    xml.pack(file)?;
    Ok(())
}

fn default_fonts(o: &Options) -> RunFonts {
    RunFonts::new()
        .ascii(&o.body_font_en)
        .hi_ansi(&o.body_font_en)
        .east_asia(&o.body_font)
        .cs(&o.body_font_en)
}

fn styles(o: &Options) -> Vec<Style> {
    let fonts = || {
        RunFonts::new()
            .ascii(&o.body_font_en)
            .hi_ansi(&o.body_font_en)
            .east_asia(&o.body_font)
            .cs(&o.body_font_en)
    };
    let mono = || {
        RunFonts::new()
            .ascii(&o.mono_font)
            .hi_ansi(&o.mono_font)
            .east_asia(&o.body_font)
            .cs(&o.mono_font)
    };

    let mut v = vec![
        Style::new("mdBody", StyleType::Paragraph)
            .name("正文")
            .based_on("Normal")
            .fonts(fonts())
            .size(o.body_size)
            .q_format(true),
        Style::new("mdCode", StyleType::Paragraph)
            .name("代码块")
            .based_on("Normal")
            .fonts(mono())
            .size(o.code_size),
        Style::new("mdQuote", StyleType::Paragraph)
            .name("引用")
            .based_on("Normal")
            .fonts(fonts())
            .size(o.body_size)
            .color(&o.quote_color),
    ];
    for i in 1..=6 {
        v.push(
            Style::new(format!("mdH{i}"), StyleType::Paragraph)
                .name(format!("标题 {i}"))
                .based_on("Normal")
                .fonts(fonts())
                .size(o.heading_sizes[i - 1])
                .color(&o.heading_color)
                .bold()
                .outline_lvl(i - 1)
                .q_format(true),
        );
    }
    v
}

fn bullet_numbering(o: &Options) -> AbstractNumbering {
    let glyphs = ["•", "○", "▪", "·", "•"];
    let fonts = || {
        RunFonts::new()
            .ascii(&o.body_font)
            .hi_ansi(&o.body_font)
            .east_asia(&o.body_font)
            .cs(&o.body_font)
    };
    let mut a = AbstractNumbering::new(1);
    for (lv, g) in glyphs.iter().enumerate() {
        a = a.add_level(
            Level::new(
                lv,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new(*g),
                LevelJc::new("left"),
            )
            .fonts(fonts())
            .indent(Some(720 + 360 * lv as i32), Some(SpecialIndentType::Hanging(360)), None, None),
        );
    }
    a
}

fn decimal_numbering(o: &Options) -> AbstractNumbering {
    let fonts = || {
        RunFonts::new()
            .ascii(&o.body_font_en)
            .hi_ansi(&o.body_font_en)
            .east_asia(&o.body_font)
            .cs(&o.body_font_en)
    };
    let mut a = AbstractNumbering::new(2);
    for lv in 0..5 {
        let text: String = (1..=lv + 1).map(|i| format!("%{i}.")).collect();
        a = a.add_level(
            Level::new(
                lv,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new(text),
                LevelJc::new("left"),
            )
            .fonts(fonts())
            .indent(Some(720 + 360 * lv as i32), Some(SpecialIndentType::Hanging(360)), None, None),
        );
    }
    a
}

// ---------------------------------------------------------------- 公式回填

/// 把占位 run 替换成真正的 OMML。
///
/// 占位 run 形如 `<w:r ...><w:t>[MD2WORDMATH:3]</w:t></w:r>`，
/// 行内公式整体换成 `<m:oMath>`，块级公式换成 `<m:oMathPara><m:oMath>`。
fn patch_math(xml: &mut XMLDocx, math: &[(String, bool)], opt: &Options) -> anyhow::Result<()> {
    if math.is_empty() {
        return Ok(());
    }
    let mut s = String::from_utf8(std::mem::take(&mut xml.document))?;
    for (id, (latex, display)) in math.iter().enumerate() {
        let token = format!("{TOKEN_PREFIX}{id}]");
        let Some(idx) = s.find(&token) else { continue };
        let Some(start) = find_run_start(&s, idx) else { continue };
        let Some(rel) = s[idx..].find("</w:r>") else { continue };
        let end = idx + rel + "</w:r>".len();
        let omml = omml::latex_to_omml(latex, &opt.math_font, *display);
        s.replace_range(start..end, &omml);
    }
    xml.document = s.into_bytes();
    Ok(())
}

/// 从 `before` 位置往前找包裹它的 `<w:r>` 起点（注意要跳过 `<w:rPr>`）
fn find_run_start(s: &str, before: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut end = before;
    loop {
        let p = s[..end].rfind("<w:r")?;
        match b.get(p + 4) {
            Some(c) if *c == b'>' || *c == b' ' => return Some(p),
            _ => {
                if p == 0 {
                    return None;
                }
                end = p;
            }
        }
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
