//! OOXML 属性元素规范化排序。
//!
//! # 为什么需要这一层
//!
//! docx-rs 的各个 `build_to` 按 Rust 字段声明顺序输出子元素，与 ECMA-376
//! 规定的 `xsd:sequence` 顺序并不一致。实测它生成的文档里：
//!
//! - `w:pPr` 把 `w:rPr` 放在**最前面**（规范要求它在最后，仅先于 `sectPr`）；
//! - `w:spacing` 排在 `w:ind` 之后（规范要求 `spacing` 在 `ind` 前）；
//! - `w:jc` 排在 `w:spacing`/`w:ind` 之前（规范要求 `jc` 在二者之后）；
//! - `w:keepNext`/`w:keepLines` 被扔到最后（规范要求它们是第 2、3 个）；
//! - `w:rPr` 把 `w:sz`/`w:szCs` 放在最前、`w:rFonts` 放在第 16 位
//!   （规范要求 `rFonts` 第 2、`sz`/`szCs` 第 23/24）；
//! - `w:numPr` 输出 `numId` 在 `ilvl` 前（规范要求 `ilvl` 在前）。
//!
//! `CT_PPrBase`、`CT_RPr` 等都是 `xsd:sequence`，顺序错误即 schema 非法。
//! Word 对乱序的容忍度并不稳定：轻则弹“文档需要修复”，重则直接拒绝打开。
//!
//! # 做法
//!
//! 在打包前，把下列属性元素的**直接子元素**按规范顺序稳定重排。
//! 只触碰列出的元素，其余字节原样保留，因此不会改变文档的任何语义。

/// 属性元素全名 → 规范子元素顺序（ECMA-376 Part 1, Transitional）
const TABLES: &[(&str, &[&str])] = &[
    ("w:pPr", &PPR),
    ("w:rPr", &RPR),
    ("w:numPr", &NUMPR),
    ("w:pBdr", &PBDR),
    ("w:tblPr", &TBLPR),
    ("w:trPr", &TRPR),
    ("w:tcPr", &TCPR),
    ("w:sectPr", &SECTPR),
    ("w:tblBorders", &TBLBORDERS),
    ("w:tcBorders", &TCBORDERS),
    ("w:tblCellMar", &CELLMAR),
    ("w:tcMar", &CELLMAR),
];

// CT_PPr = CT_PPrBase + rPr + sectPr + pPrChange
const PPR: &[&str] = &[
    "w:pStyle", "w:keepNext", "w:keepLines", "w:pageBreakBefore", "w:framePr",
    "w:widowControl", "w:numPr", "w:suppressLineNumbers", "w:pBdr", "w:shd",
    "w:tabs", "w:suppressAutoHyphens", "w:kinsoku", "w:wordWrap",
    "w:overflowPunct", "w:topLinePunct", "w:autoSpaceDE", "w:autoSpaceDN",
    "w:bidi", "w:adjustRightInd", "w:snapToGrid", "w:spacing", "w:ind",
    "w:contextualSpacing", "w:mirrorIndents", "w:suppressOverlap", "w:jc",
    "w:textDirection", "w:textAlignment", "w:textboxTightWrap",
    "w:outlineLvl", "w:divId", "w:cnfStyle", "w:rPr", "w:sectPr", "w:pPrChange",
];

// CT_RPr（CT_ParaRPr 追加的 ins/del/moveFrom/moveTo/rPrChange 会落到末尾）
const RPR: &[&str] = &[
    "w:rStyle", "w:rFonts", "w:b", "w:bCs", "w:i", "w:iCs", "w:caps",
    "w:smallCaps", "w:strike", "w:dstrike", "w:outline", "w:shadow",
    "w:emboss", "w:imprint", "w:noProof", "w:snapToGrid", "w:vanish",
    "w:webHidden", "w:color", "w:spacing", "w:w", "w:kern", "w:position",
    "w:sz", "w:szCs", "w:highlight", "w:u", "w:effect", "w:bdr", "w:shd",
    "w:fitText", "w:vertAlign", "w:rtl", "w:cs", "w:em", "w:lang",
    "w:eastAsianLayout", "w:specVanish", "w:oMath",
];

const NUMPR: &[&str] = &["w:ilvl", "w:numId", "w:numPrChange"];
const PBDR: &[&str] = &["w:top", "w:left", "w:bottom", "w:right", "w:between", "w:bar"];

const TBLPR: &[&str] = &[
    "w:tblStyle", "w:tblpPr", "w:tblOverlap", "w:bidiVisual",
    "w:tblStyleRowBandSize", "w:tblStyleColBandSize", "w:tblW", "w:jc",
    "w:tblCellSpacing", "w:tblInd", "w:tblBorders", "w:shd", "w:tblLayout",
    "w:tblCellMar", "w:tblLook", "w:tblCaption", "w:tblDescription",
    "w:tblPrChange",
];

const TRPR: &[&str] = &[
    "w:cnfStyle", "w:divId", "w:gridBefore", "w:gridAfter", "w:wBefore",
    "w:wAfter", "w:cantSplit", "w:trHeight", "w:tblHeader", "w:tblCellSpacing",
    "w:jc", "w:hidden", "w:ins", "w:del", "w:trPrChange",
];

const TCPR: &[&str] = &[
    "w:cnfStyle", "w:tcW", "w:gridSpan", "w:hMerge", "w:vMerge", "w:tcBorders",
    "w:shd", "w:noWrap", "w:tcMar", "w:textDirection", "w:tcFitText",
    "w:vAlign", "w:hideMark", "w:headers", "w:cellIns", "w:cellDel",
    "w:cellMerge", "w:tcPrChange",
];

const SECTPR: &[&str] = &[
    "w:footnotePr", "w:endnotePr", "w:type", "w:pgSz", "w:pgMar", "w:paperSrc",
    "w:pgBorders", "w:lnNumType", "w:pgNumType", "w:cols", "w:formProt",
    "w:vAlign", "w:noEndnote", "w:titlePg", "w:textDirection", "w:bidi",
    "w:rtlGutter", "w:docGrid", "w:printerSettings", "w:sectPrChange",
];

const TBLBORDERS: &[&str] = &[
    "w:top", "w:start", "w:left", "w:bottom", "w:end", "w:right", "w:insideH",
    "w:insideV",
];

const TCBORDERS: &[&str] = &[
    "w:top", "w:start", "w:left", "w:bottom", "w:end", "w:right", "w:insideH",
    "w:insideV", "w:tl2br", "w:tr2bl",
];

const CELLMAR: &[&str] = &["w:top", "w:start", "w:left", "w:bottom", "w:end", "w:right"];

/// 对一段 XML 做属性元素子元素排序。
pub fn normalize(xml: &str) -> String {
    let mut s = xml.to_string();
    for (tag, order) in TABLES {
        s = reorder(&s, tag, order);
    }
    s
}

/// `pos` 处是否确实是名为 `tag` 的元素开标签（排除 `<w:pPrChange`
/// 这类以 `<w:pPr` 为前缀的长名字）。
fn is_boundary(s: &str, pos: usize, tag: &str) -> bool {
    match s.as_bytes().get(pos + 1 + tag.len()) {
        None | Some(b'>') | Some(b'/') => true,
        Some(c) => c.is_ascii_whitespace(),
    }
}

/// 开标签的结束 `>` 下标，跳过属性值里的引号。
fn tag_end(s: &str, pos: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = pos + 1;
    let mut quote = 0u8;
    while i < b.len() {
        match b[i] {
            q if quote == 0 && (q == b'"' || q == b'\'') => quote = q,
            q if q == quote => quote = 0,
            b'>' if quote == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// 从 `from` 起找与 `pos` 配对的 `</tag>` 起始下标。
///
/// 注意：这里必须按**字节**比较。逐字节前进时下标可能落在多字节 UTF-8
/// 字符中间，此时对 `&str` 切片会直接 panic（中文字符占 3 字节）。
fn find_close(s: &str, from: usize, tag: &str) -> Option<usize> {
    let b = s.as_bytes();
    let close = format!("</{tag}>").into_bytes();
    let open = format!("<{tag}").into_bytes();
    let mut depth = 1usize;
    let mut i = from;
    while i < b.len() {
        if b[i..].starts_with(&close) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += close.len();
        } else if b[i..].starts_with(&open) && is_boundary(s, i, tag) {
            match tag_end(s, i) {
                Some(e) => {
                    // 自闭合标签不改变深度
                    if b[e - 1] != b'/' {
                        depth += 1;
                    }
                    i = e + 1;
                }
                None => return None,
            }
        } else {
            i += 1;
        }
    }
    None
}

/// 子节点在规范顺序表中的下标；未收录的排在最后（稳定排序保持原相对次序）。
fn order_index(child: &str, order: &[&str]) -> usize {
    let name = child
        .strip_prefix('<')
        .and_then(|rest| {
            let end = rest
                .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
                .unwrap_or(rest.len());
            Some(&rest[..end])
        })
        .unwrap_or("");
    order.iter().position(|t| *t == name).unwrap_or(order.len())
}

/// 把元素内部 XML 拆成直接子节点列表（保留原始文本）。
fn split_children(inner: &str) -> Vec<String> {
    let b = inner.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let start = i;
            while i < b.len() && b[i] != b'<' {
                i += 1;
            }
            out.push(inner[start..i].to_string());
            continue;
        }
        let start = i;
        if inner[i..].starts_with("<!--") {
            i = inner[i..].find("-->").map(|p| i + p + 3).unwrap_or(b.len());
        } else if inner[i..].starts_with("<![CDATA[") {
            i = inner[i..].find("]]>").map(|p| i + p + 3).unwrap_or(b.len());
        } else if inner[i..].starts_with("<?") || inner[i..].starts_with("<!") {
            i = inner[i..].find('>').map(|p| i + p + 1).unwrap_or(b.len());
        } else {
            match tag_end(inner, i) {
                Some(e) => {
                    if b[e - 1] == b'/' {
                        i = e + 1; // 自闭合
                    } else {
                        // 取出元素名，找配对闭合标签
                        let name_end = inner[i + 1..e]
                            .find(|c: char| c.is_ascii_whitespace() || c == '/')
                            .map(|p| i + 1 + p)
                            .unwrap_or(e);
                        let name = &inner[i + 1..name_end];
                        let close = format!("</{name}>").into_bytes();
                        let open = format!("<{name}").into_bytes();
                        let mut depth = 1usize;
                        let mut j = e + 1;
                        while j < b.len() {
                            if b[j..].starts_with(&close) {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                j += close.len();
                            } else if b[j..].starts_with(&open) {
                                match tag_end(inner, j) {
                                    Some(e2) => {
                                        if b[e2 - 1] != b'/' {
                                            depth += 1;
                                        }
                                        j = e2 + 1;
                                    }
                                    None => break,
                                }
                            } else {
                                j += 1;
                            }
                        }
                        i = (j + close.len()).min(b.len());
                    }
                }
                None => i = b.len(),
            }
        }
        out.push(inner[start..i.min(b.len())].to_string());
    }
    out
}

/// 重排所有 `tag` 元素的直接子元素。
fn reorder(s: &str, tag: &str, order: &[&str]) -> String {
    let needle = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let rel = match s[i..].find(&needle) {
            None => {
                out.push_str(&s[i..]);
                break;
            }
            Some(r) => r,
        };
        let pos = i + rel;
        if !is_boundary(s, pos, tag) {
            out.push_str(&s[i..pos + 1]);
            i = pos + 1;
            continue;
        }
        let open_end = match tag_end(s, pos) {
            Some(e) => e,
            None => {
                out.push_str(&s[i..]);
                break;
            }
        };
        out.push_str(&s[i..=open_end]);
        // 自闭合元素没有子元素
        if s.as_bytes()[open_end - 1] == b'/' {
            i = open_end + 1;
            continue;
        }
        let close_pos = match find_close(s, open_end + 1, tag) {
            Some(p) => p,
            None => {
                out.push_str(&s[open_end + 1..]);
                break;
            }
        };
        let mut kids = split_children(&s[open_end + 1..close_pos]);
        kids.sort_by_key(|k| order_index(k, order));
        for k in kids {
            out.push_str(&k);
        }
        out.push_str(&close);
        i = close_pos + close.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppr_rpr_moves_to_end() {
        let src = r#"<w:pPr><w:rPr /><w:pStyle w:val="mdBody" /><w:spacing w:line="360" /></w:pPr>"#;
        let out = reorder(src, "w:pPr", PPR);
        assert_eq!(
            out,
            r#"<w:pPr><w:pStyle w:val="mdBody" /><w:spacing w:line="360" /><w:rPr /></w:pPr>"#
        );
    }

    #[test]
    fn rpr_reordered() {
        let src = r#"<w:rPr><w:sz w:val="24" /><w:szCs w:val="24" /><w:color w:val="1F3864" /><w:b /><w:rFonts w:ascii="Times New Roman" /></w:rPr>"#;
        let out = reorder(src, "w:rPr", RPR);
        assert_eq!(
            out,
            r#"<w:rPr><w:rFonts w:ascii="Times New Roman" /><w:b /><w:color w:val="1F3864" /><w:sz w:val="24" /><w:szCs w:val="24" /></w:rPr>"#
        );
    }

    #[test]
    fn numpr_ilvl_first() {
        let src = r#"<w:numPr><w:numId w:val="2" /><w:ilvl w:val="0" /></w:numPr>"#;
        let out = reorder(src, "w:numPr", NUMPR);
        assert_eq!(
            out,
            r#"<w:numPr><w:ilvl w:val="0" /><w:numId w:val="2" /></w:numPr>"#
        );
    }

    #[test]
    fn prefix_collision_not_matched() {
        // <w:pPrChange 不能被当成 <w:pPr 处理
        let src = r#"<w:pPrChange><w:rPr /></w:pPrChange>"#;
        let out = reorder(src, "w:pPr", PPR);
        assert_eq!(out, src);
    }

    #[test]
    fn nested_and_text_preserved() {
        let src = r#"<w:pPr><w:rPr><w:sz w:val="24" /><w:rFonts w:ascii="A" /></w:rPr><w:pStyle w:val="x" /></w:pPr>"#;
        let out = normalize(src);
        assert_eq!(
            out,
            r#"<w:pPr><w:pStyle w:val="x" /><w:rPr><w:rFonts w:ascii="A" /><w:sz w:val="24" /></w:rPr></w:pPr>"#
        );
    }

    #[test]
    fn self_closing_untouched() {
        let src = r#"<w:pPr><w:rPr /></w:pPr>"#;
        assert_eq!(reorder(src, "w:pPr", PPR), src);
    }

    #[test]
    fn escaped_text_not_a_tag() {
        let src = r#"<w:p><w:r><w:t>a &lt; b &amp; c</w:t></w:r></w:p>"#;
        assert_eq!(normalize(src), src);
    }
}
