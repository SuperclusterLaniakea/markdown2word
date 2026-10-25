//! LaTeX -> OMML（Office Math Markup Language）
//!
//! Word 里的公式不是文本，而是一棵结构树：分数是 `<m:f>`、根式是 `<m:rad>`、
//! 求和积分是 `<m:nary>`、括号是 `<m:d>`、矩阵是 `<m:m>`。
//! 所以这里先把 LaTeX 解析成一棵 `N` 语法树（递归下降），再逐节点映射成 OMML 元素。
//!
//! 覆盖日常与论文场景的常用子集；遇到不认识的宏，降级成"正体字面量"而不是丢弃，
//! 保证公式内容不丢。

// ---------------------------------------------------------------- 语法树

#[derive(Debug, Clone)]
enum N {
    Seq(Vec<N>),
    /// (文本, 是否正体 upright)
    Txt(String, bool),
    Frac(Box<N>, Box<N>),
    Sqrt(Option<Box<N>>, Box<N>),
    Sup(Box<N>, Box<N>),
    Sub(Box<N>, Box<N>),
    SubSup(Box<N>, Box<N>, Box<N>),
    /// 求和/积分：算符字符、下标、上标、被作用体
    Nary(String, Option<Box<N>>, Option<Box<N>>, Box<N>),
    /// 函数名 + 参数（sin x / log_2 n）
    Func(String, Box<N>),
    /// 定界符包裹（\left( ... \right) 或普通括号）
    Delim(String, String, Box<N>),
    Matrix(Vec<Vec<N>>),
    /// 多行对齐公式（aligned / gather / split）
    EqArr(Vec<Vec<N>>),
    Acc(String, Box<N>),
    /// true = 上划线，false = 下划线
    Bar(bool, Box<N>),
}

// ---------------------------------------------------------------- 词法

#[derive(Debug, Clone, PartialEq)]
enum T {
    Cmd(String),
    Ch(char),
    Lb,
    Rb,
    Sup,
    Sub,
    Amp,
    Newline,
    Lbr,
    Rbr,
}

fn tokenize(src: &str) -> Vec<T> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '\\' => {
                i += 1;
                if i >= cs.len() {
                    break;
                }
                let n = cs[i];
                if n == '\\' {
                    out.push(T::Newline);
                    i += 1;
                } else if n.is_ascii_alphabetic() {
                    let start = i;
                    while i < cs.len() && cs[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    out.push(T::Cmd(cs[start..i].iter().collect()));
                } else {
                    out.push(T::Cmd(n.to_string()));
                    i += 1;
                }
            }
            '{' => { out.push(T::Lb); i += 1; }
            '}' => { out.push(T::Rb); i += 1; }
            '^' => { out.push(T::Sup); i += 1; }
            '_' => { out.push(T::Sub); i += 1; }
            '&' => { out.push(T::Amp); i += 1; }
            '[' => { out.push(T::Lbr); i += 1; }
            ']' => { out.push(T::Rbr); i += 1; }
            '%' => { while i < cs.len() && cs[i] != '\n' { i += 1; } }
            c if c.is_whitespace() => i += 1,
            c => { out.push(T::Ch(c)); i += 1; }
        }
    }
    out
}

// ---------------------------------------------------------------- 语法

struct P {
    t: Vec<T>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&T> {
        self.t.get(self.i)
    }

    /// 解析到 `}` / `&` / `\\` / 结束 为止
    fn seq(&mut self) -> Vec<N> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None | Some(T::Rb) | Some(T::Amp) | Some(T::Newline) => break,
                _ => out.push(self.atom()),
            }
        }
        out
    }

    /// 解析到 `\right` / `}` / 结束 为止
    fn seq_until_right(&mut self) -> Vec<N> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None | Some(T::Rb) => break,
                Some(T::Cmd(n)) if n == "right" || n == "end" => break,
                _ => out.push(self.atom()),
            }
        }
        out
    }

    /// 一个参数：`{...}` 整体，或单个原子（不带后缀）
    fn arg(&mut self) -> N {
        if matches!(self.peek(), Some(T::Lb)) {
            self.i += 1;
            let v = self.seq();
            if matches!(self.peek(), Some(T::Rb)) {
                self.i += 1;
            }
            N::Seq(v)
        } else {
            self.atom_base()
        }
    }

    fn atom(&mut self) -> N {
        let base = self.atom_base();
        self.with_suffixes(base)
    }

    fn atom_base(&mut self) -> N {
        let tok = self.peek().cloned();
        match tok {
            None => N::Seq(vec![]),
            Some(T::Lb) => {
                self.i += 1;
                let v = self.seq();
                if matches!(self.peek(), Some(T::Rb)) {
                    self.i += 1;
                }
                N::Seq(v)
            }
            Some(T::Lbr) => {
                self.i += 1;
                let v = self.seq();
                if matches!(self.peek(), Some(T::Rbr)) {
                    self.i += 1;
                }
                N::Seq(v)
            }
            Some(T::Cmd(n)) => {
                self.i += 1;
                self.cmd(&n)
            }
            Some(T::Ch(c)) => {
                self.i += 1;
                self.ch_atom(c)
            }
            Some(T::Sup) => {
                self.i += 1;
                N::Sup(Box::new(N::Seq(vec![])), Box::new(self.arg()))
            }
            Some(T::Sub) => {
                self.i += 1;
                N::Sub(Box::new(N::Seq(vec![])), Box::new(self.arg()))
            }
            Some(_) => {
                self.i += 1;
                N::Seq(vec![])
            }
        }
    }

    /// 普通字符：`(`/`[` 尝试配成一对定界符，其余按字面量
    fn ch_atom(&mut self, c: char) -> N {
        match c {
            '(' => self.paren('(', ')'),
            '[' => self.paren('[', ']'),
            _ => N::Txt(c.to_string(), false),
        }
    }

    fn paren(&mut self, open: char, close: char) -> N {
        let start = self.i;
        let mut depth = 1i32;
        let mut end = self.t.len();
        while self.i < self.t.len() {
            if let T::Ch(c) = self.t[self.i] {
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        end = self.i;
                        break;
                    }
                }
            }
            self.i += 1;
        }
        let body = {
            let sub = P { t: self.t[start..end].to_vec(), i: 0 };
            sub.parse_all()
        };
        self.i = end;
        if self.i < self.t.len() {
            self.i += 1; // 跳过右定界符
        }
        N::Delim(open.to_string(), close.to_string(), Box::new(body))
    }

    /// 解析整个 token 序列（供子解析器使用）
    fn parse_all(mut self) -> N {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => break,
                Some(T::Rb) => { self.i += 1; }
                _ => out.push(self.atom()),
            }
        }
        N::Seq(out)
    }

    fn with_suffixes(&mut self, base: N) -> N {
        let mut sub: Option<Box<N>> = None;
        let mut sup: Option<Box<N>> = None;
        loop {
            match self.peek() {
                Some(T::Sup) => { self.i += 1; sup = Some(Box::new(self.arg())); }
                Some(T::Sub) => { self.i += 1; sub = Some(Box::new(self.arg())); }
                _ => break,
            }
        }
        match (sub, sup) {
            (Some(s), Some(p)) => N::SubSup(Box::new(base), s, p),
            (Some(s), None) => N::Sub(Box::new(base), s),
            (None, Some(p)) => N::Sup(Box::new(base), p),
            (None, None) => base,
        }
    }

    /// 读取 `\sum_{a}^{b}` 这类紧跟的上下限
    fn limits(&mut self) -> (Option<Box<N>>, Option<Box<N>>) {
        let mut sub = None;
        let mut sup = None;
        loop {
            match self.peek() {
                Some(T::Sub) => { self.i += 1; sub = Some(Box::new(self.arg())); }
                Some(T::Sup) => { self.i += 1; sup = Some(Box::new(self.arg())); }
                _ => break,
            }
        }
        (sub, sup)
    }

    /// 取下一个原子作为"被作用体"，但不吞掉分隔符/结束符
    fn body_arg(&mut self) -> Box<N> {
        match self.peek() {
            None | Some(T::Rb) | Some(T::Amp) | Some(T::Newline) => Box::new(N::Seq(vec![])),
            Some(T::Sup) | Some(T::Sub) => Box::new(N::Seq(vec![])),
            Some(T::Cmd(n)) if n == "right" || n == "end" => Box::new(N::Seq(vec![])),
            _ => Box::new(self.arg()),
        }
    }

    /// 读取 `{...}` 内的原始文本（用于 \text 等）
    fn raw_group(&mut self) -> String {
        match self.peek().cloned() {
            Some(T::Lb) => {
                self.i += 1;
                let mut depth = 1i32;
                let mut s = String::new();
                while self.i < self.t.len() {
                    match &self.t[self.i] {
                        T::Lb => { depth += 1; s.push('{'); self.i += 1; }
                        T::Rb => {
                            depth -= 1;
                            self.i += 1;
                            if depth == 0 { break; }
                            s.push('}');
                        }
                        T::Cmd(n) => { s.push_str(n); self.i += 1; }
                        T::Ch(c) => { s.push(*c); self.i += 1; }
                        T::Newline => { self.i += 1; }
                        _ => { self.i += 1; }
                    }
                }
                s
            }
            Some(T::Cmd(n)) => { self.i += 1; n }
            Some(T::Ch(c)) => { self.i += 1; c.to_string() }
            _ => String::new(),
        }
    }

    fn delim(&mut self) -> String {
        match self.peek().cloned() {
            Some(T::Cmd(n)) => {
                self.i += 1;
                if n == "." { String::new() } else { map_delim(&n) }
            }
            Some(T::Ch(c)) => { self.i += 1; c.to_string() }
            _ => String::new(),
        }
    }

    fn cmd(&mut self, name: &str) -> N {
        match name {
            // ---- 分数 / 二项式
            "frac" | "dfrac" | "tfrac" | "cfrac" | "nicefrac" => {
                let a = self.arg();
                let b = self.arg();
                N::Frac(Box::new(a), Box::new(b))
            }
            "binom" | "dbinom" | "tbinom" => {
                let a = self.arg();
                let b = self.arg();
                N::Delim("(".into(), ")".into(), Box::new(N::Frac(Box::new(a), Box::new(b))))
            }
            // ---- 根式
            "sqrt" => {
                let deg = if matches!(self.peek(), Some(T::Lbr)) {
                    self.i += 1;
                    let d = self.seq();
                    if matches!(self.peek(), Some(T::Rbr)) {
                        self.i += 1;
                    }
                    Some(Box::new(N::Seq(d)))
                } else {
                    None
                };
                N::Sqrt(deg, Box::new(self.arg()))
            }
            // ---- 文本 / 字体样式（降级为正体字面量）
            "text" | "textrm" | "textsf" | "texttt" | "textbf" | "textit" | "textsc"
            | "mathrm" | "mbox" | "hbox" | "mathbf" | "boldsymbol" | "bm" | "mathit"
            | "mathsf" | "mathtt" | "rm" | "bf" => {
                let s = self.raw_group();
                N::Txt(s, true)
            }
            "mathbb" => { let s = self.raw_group(); N::Txt(blackboard(&s), true) }
            "mathcal" | "cal" => { let s = self.raw_group(); N::Txt(script(&s), true) }
            "mathfrak" => { let s = self.raw_group(); N::Txt(fraktur(&s), true) }
            "operatorname" | "operatorname*" => {
                let s = self.raw_group();
                N::Func(s, self.body_arg())
            }
            // ---- \left ... \right
            "left" => {
                let beg = self.delim();
                let body = self.seq_until_right();
                let end = if matches!(self.peek(), Some(T::Cmd(n)) if n == "right") {
                    self.i += 1;
                    self.delim()
                } else {
                    String::new()
                };
                N::Delim(beg, end, Box::new(N::Seq(body)))
            }
            "right" | "middle" => {
                let _ = self.delim();
                N::Seq(vec![])
            }
            // ---- 环境
            "begin" => {
                let env = self.raw_group();
                if env == "array" && matches!(self.peek(), Some(T::Lb)) {
                    let _ = self.raw_group(); // 吃掉 {cc} 列格式
                }
                let rows = self.matrix(&env);
                env_node(&env, rows)
            }
            "end" => {
                let _ = self.raw_group();
                N::Seq(vec![])
            }
            // ---- 上下标结构
            "overbrace" => N::Acc("⏞".into(), Box::new(self.arg())),
            "underbrace" => N::Acc("⏟".into(), Box::new(self.arg())),
            "overline" => N::Bar(true, Box::new(self.arg())),
            "underline" => N::Bar(false, Box::new(self.arg())),
            "hat" | "widehat" => N::Acc("^".into(), Box::new(self.arg())),
            "bar" => N::Acc("¯".into(), Box::new(self.arg())),
            "vec" => N::Acc("⃗".into(), Box::new(self.arg())),
            "tilde" | "widetilde" => N::Acc("~".into(), Box::new(self.arg())),
            "dot" => N::Acc("˙".into(), Box::new(self.arg())),
            "ddot" => N::Acc("¨".into(), Box::new(self.arg())),
            "dddot" => N::Acc("⃛".into(), Box::new(self.arg())),
            "check" => N::Acc("ˇ".into(), Box::new(self.arg())),
            "breve" => N::Acc("˘".into(), Box::new(self.arg())),
            "acute" => N::Acc("´".into(), Box::new(self.arg())),
            "grave" => N::Acc("`".into(), Box::new(self.arg())),
            "mathring" => N::Acc("˚".into(), Box::new(self.arg())),
            "overrightarrow" | "overrightharpoon" => N::Acc("⇀".into(), Box::new(self.arg())),
            "overleftarrow" | "overleftharpoon" => N::Acc("↼".into(), Box::new(self.arg())),
            "overset" | "stackrel" => {
                let top = self.arg();
                let base = self.arg();
                N::Sup(Box::new(base), Box::new(top))
            }
            "underset" => {
                let bot = self.arg();
                let base = self.arg();
                N::Sub(Box::new(base), Box::new(bot))
            }
            "not" => {
                let a = self.arg();
                let s = match &a {
                    N::Txt(t, _) => match t.trim() {
                        "=" => "≠", "<" => "≮", ">" => "≯", "∈" => "∉", "⊂" => "⊄",
                        "⊆" => "⊈", "≡" => "≢", "≤" => "≰", "≥" => "≱", "∣" => "∤",
                        _ => "¬",
                    },
                    _ => "¬",
                };
                if s == "¬" {
                    N::Seq(vec![N::Txt("¬".into(), false), a])
                } else {
                    N::Txt(s.into(), false)
                }
            }
            // ---- 空间与修饰命令：吞掉参数，不产生内容
            "hspace" | "mspace" | "kern" | "mkern" | "phantom" | "hphantom" | "vphantom"
            | "label" | "tag" | "ref" | "eqref" | "cite" | "citep" | "citet" | "nonumber"
            | "notag" | "mathstrut" | "smash" | "rlap" | "llap" | "mathrlap" => {
                let _ = self.arg();
                N::Seq(vec![])
            }
            "color" => { let _ = self.arg(); N::Seq(vec![]) }
            "substack" => self.arg(),
            // ---- 间距
            "quad" | "qquad" | "!" | "," | ";" | ":" | " " | "thinspace" | "medspace"
            | "thickspace" | "enspace" | "negthinspace" | "negmedspace" => {
                N::Txt(" ".into(), false)
            }
            "pmod" => { let s = self.raw_group(); N::Txt(format!(" (mod {s})"), true) }
            "bmod" | "mod" => N::Txt(" mod ".into(), true),
            "colon" => N::Txt(":".into(), false),
            // ---- \big 系列：只保留定界符本身
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "Bigl" | "biggl" | "Biggl" | "bigr"
            | "Bigr" | "biggr" | "Biggr" | "left." => N::Txt(self.delim(), false),
            // ---- 极限类函数（\lim_{x \to 0}）
            "lim" | "limsup" | "liminf" | "max" | "min" | "sup" | "inf" | "argmax"
            | "argmin" | "Pr" => self.limit_func(name),
            // ---- 命名函数（\sin x）
            "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "arcsin" | "arccos" | "arctan"
            | "arccot" | "sinh" | "cosh" | "tanh" | "coth" | "exp" | "log" | "ln" | "lg"
            | "det" | "dim" | "gcd" | "lcm" | "hom" | "ker" | "deg" | "arg" | "sgn" => {
                self.named_func(name)
            }
            // ---- 求和 / 积分类
            "sum" | "prod" | "coprod" | "int" | "iint" | "iiint" | "iiiint" | "oint"
            | "oiint" | "oiiint" | "bigcup" | "bigcap" | "bigoplus" | "bigotimes"
            | "bigodot" | "biguplus" | "bigsqcup" | "bigvee" | "bigwedge" => {
                let chr = nary_chr(name);
                let (sub, sup) = self.limits();
                let body = self.body_arg();
                N::Nary(chr.into(), sub, sup, body)
            }
            "limits" | "nolimits" | "displaystyle" | "textstyle" | "scriptstyle"
            | "scriptscriptstyle" | "noalign" => N::Seq(vec![]),
            _ => match symbol(name) {
                Some(s) => N::Txt(s.into(), false),
                // 不认识的宏：保留名字（正体），至少不丢内容
                None => N::Txt(name.to_string(), true),
            },
        }
    }

    fn limit_func(&mut self, name: &str) -> N {
        let (sub, sup) = self.limits();
        let body = self.body_arg();
        match (sub, sup) {
            (None, None) => N::Func(name.to_string(), body),
            (Some(s), None) => {
                N::Seq(vec![N::Sub(Box::new(N::Txt(name.to_string(), true)), s), *body])
            }
            (None, Some(p)) => {
                N::Seq(vec![N::Sup(Box::new(N::Txt(name.to_string(), true)), p), *body])
            }
            (Some(s), Some(p)) => {
                N::Seq(vec![N::SubSup(Box::new(N::Txt(name.to_string(), true)), s, p), *body])
            }
        }
    }

    fn named_func(&mut self, name: &str) -> N {
        let (sub, sup) = self.limits();
        let body = self.body_arg();
        match (sub, sup) {
            (None, None) => N::Func(name.to_string(), body),
            (Some(s), None) => N::Seq(vec![
                N::Sub(Box::new(N::Txt(name.to_string(), true)), s),
                *body,
            ]),
            (None, Some(p)) => N::Seq(vec![
                N::Sup(Box::new(N::Txt(name.to_string(), true)), p),
                *body,
            ]),
            (Some(s), Some(p)) => N::Seq(vec![
                N::SubSup(Box::new(N::Txt(name.to_string(), true)), s, p),
                *body,
            ]),
        }
    }

    /// 读取矩阵/对齐环境：`&` 分列，`\\` 分行
    fn matrix(&mut self, _env: &str) -> Vec<Vec<N>> {
        let mut rows: Vec<Vec<N>> = Vec::new();
        let mut row: Vec<N> = Vec::new();
        let mut cell: Vec<N> = Vec::new();
        loop {
            if self.i >= self.t.len() {
                break;
            }
            match &self.t[self.i] {
                T::Cmd(n) if n == "end" => {
                    self.i += 1;
                    if matches!(self.peek(), Some(T::Lb)) {
                        let _ = self.raw_group();
                    }
                    break;
                }
                T::Cmd(n) if n == "right" => break,
                T::Amp => {
                    self.i += 1;
                    row.push(N::Seq(std::mem::take(&mut cell)));
                }
                T::Newline => {
                    self.i += 1;
                    row.push(N::Seq(std::mem::take(&mut cell)));
                    rows.push(std::mem::take(&mut row));
                }
                _ => cell.push(self.atom()),
            }
        }
        if !cell.is_empty() || !row.is_empty() {
            row.push(N::Seq(std::mem::take(&mut cell)));
        }
        if !row.is_empty() {
            rows.push(row);
        }
        rows
    }
}

fn env_node(env: &str, rows: Vec<Vec<N>>) -> N {
    match env {
        "matrix" | "smallmatrix" | "array" | "subarray" => N::Matrix(rows),
        "pmatrix" => N::Delim("(".into(), ")".into(), Box::new(N::Matrix(rows))),
        "bmatrix" => N::Delim("[".into(), "]".into(), Box::new(N::Matrix(rows))),
        "Bmatrix" => N::Delim("{".into(), "}".into(), Box::new(N::Matrix(rows))),
        "vmatrix" => N::Delim("|".into(), "|".into(), Box::new(N::Matrix(rows))),
        "Vmatrix" => N::Delim("‖".into(), "‖".into(), Box::new(N::Matrix(rows))),
        "cases" | "dcases" | "rcases" => {
            N::Delim("{".into(), String::new(), Box::new(N::Matrix(rows)))
        }
        "aligned" | "align" | "align*" | "alignedat" | "split" | "gather" | "gathered"
        | "eqnarray" | "eqnarray*" | "multline" | "multline*" => N::EqArr(rows),
        _ => N::Matrix(rows),
    }
}

// ---------------------------------------------------------------- 符号表

fn map_delim(n: &str) -> String {
    match n {
        "{" | "}" | "|" => n.to_string(),
        "lvert" | "rvert" | "vert" => "|".into(),
        "lVert" | "rVert" | "Vert" => "‖".into(),
        "langle" => "⟨".into(),
        "rangle" => "⟩".into(),
        "lceil" => "⌈".into(),
        "rceil" => "⌉".into(),
        "lfloor" => "⌊".into(),
        "rfloor" => "⌋".into(),
        "llbracket" => "⟦".into(),
        "rrbracket" => "⟧".into(),
        "backslash" => "\\".into(),
        "uparrow" => "↑".into(),
        "downarrow" => "↓".into(),
        "updownarrow" => "↕".into(),
        "Uparrow" => "⇑".into(),
        "Downarrow" => "⇓".into(),
        "Updownarrow" => "⇕".into(),
        _ => symbol(n).unwrap_or("|").to_string(),
    }
}

fn nary_chr(n: &str) -> &'static str {
    match n {
        "sum" => "∑",
        "prod" => "∏",
        "coprod" => "∐",
        "int" => "∫",
        "iint" => "∬",
        "iiint" => "∭",
        "iiiint" => "⨌",
        "oint" => "∮",
        "oiint" => "∯",
        "oiiint" => "∰",
        "bigcup" => "⋃",
        "bigcap" => "⋂",
        "bigoplus" => "⨁",
        "bigotimes" => "⨂",
        "bigodot" => "⨀",
        "biguplus" => "⨄",
        "bigsqcup" => "⨆",
        "bigvee" => "⋁",
        "bigwedge" => "⋀",
        _ => "∑",
    }
}

fn symbol(n: &str) -> Option<&'static str> {
    let s = match n {
        // 希腊字母（小写）
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ",
        "epsilon" => "ϵ", "varepsilon" => "ε", "zeta" => "ζ", "eta" => "η",
        "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ",
        "lambda" => "λ", "mu" => "μ", "nu" => "ν", "xi" => "ξ", "pi" => "π",
        "varpi" => "ϖ", "rho" => "ρ", "varrho" => "ϱ", "sigma" => "σ",
        "varsigma" => "ς", "tau" => "τ", "upsilon" => "υ", "phi" => "ϕ",
        "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "digamma" => "ϝ", "varkappa" => "ϰ",
        // 希腊字母（大写）
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ",
        "Xi" => "Ξ", "Pi" => "Π", "Sigma" => "Σ", "Upsilon" => "Υ",
        "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω",
        // 二元运算符
        "times" => "×", "div" => "÷", "pm" => "±", "mp" => "∓", "cdot" => "⋅",
        "ast" => "∗", "star" => "⋆", "circ" => "∘", "bullet" => "∙",
        "oplus" => "⊕", "ominus" => "⊖", "otimes" => "⊗", "oslash" => "⊘",
        "odot" => "⊙", "dagger" => "†", "ddagger" => "‡", "amalg" => "⨿",
        "wr" => "≀", "diamond" => "⋄", "sqcup" => "⊔", "sqcap" => "⊓",
        "uplus" => "⊎", "setminus" => "∖", "smallsetminus" => "∖",
        "cap" => "∩", "cup" => "∪", "land" => "∧", "lor" => "∨", "wedge" => "∧",
        "vee" => "∨", "veebar" => "⊻", "barwedge" => "⌅", "doublebarwedge" => "⌆",
        "ltimes" => "⋉", "rtimes" => "⋊", "Join" => "⋈", "bowtie" => "⋈",
        "divideontimes" => "⋇", "centerdot" => "⋅", "intercal" => "⊺",
        // 关系符
        "leq" | "le" => "≤", "geq" | "ge" => "≥", "ll" => "≪", "gg" => "≫",
        "prec" => "≺", "succ" => "≻", "preceq" => "⪯", "succeq" => "⪰",
        "sim" => "∼", "simeq" => "≃", "asymp" => "≍", "approx" => "≈",
        "cong" => "≅", "neq" | "ne" => "≠", "equiv" => "≡", "propto" => "∝",
        "models" => "⊨", "perp" => "⊥", "mid" => "∣", "parallel" => "∥",
        "smile" => "⌣", "frown" => "⌢", "doteq" => "≐", "precsim" => "≾",
        "succsim" => "≿", "lesssim" => "≲", "gtrsim" => "≳", "subseteq" => "⊆",
        "supseteq" => "⊇", "subset" => "⊂", "supset" => "⊃", "subsetneq" => "⊊",
        "supsetneq" => "⊋", "in" => "∈", "notin" => "∉", "ni" => "∋",
        "vdash" => "⊢", "dashv" => "⊣", "nleq" => "≰", "ngeq" => "≱",
        "nmid" => "∤", "nequiv" => "≢", "nsim" => "≁", "ncong" => "≇",
        "napprox" => "≉", "lneq" => "≨", "gneq" => "≩", "coloneq" | "coloneqq" => "≔",
        "eqcolon" => "≕", "colonequals" => "≔", "lessdot" => "⋖", "gtrdot" => "⋗",
        "lll" => "⋘", "ggg" => "⋙", "lessgtr" => "≶", "gtrless" => "≷",
        "eqslantless" => "⋜", "eqslantgtr" => "⋝", "thickapprox" => "≈",
        "thicksim" => "∼", "trianglelefteq" => "⊴", "trianglerighteq" => "⊵",
        "vartriangleleft" => "⊲", "vartriangleright" => "⊳", "multimap" => "⊸",
        "Vdash" => "⊩", "Vvdash" => "⊫", "bumpeq" => "≏", "Bumpeq" => "≎",
        "geqq" => "≧", "leqq" => "≦", "geqslant" => "⩾", "leqslant" => "⩽",
        "eqcirc" => "≖", "circeq" => "≗", "triangleq" => "≜",
        "risingdotseq" => "≒", "fallingdotseq" => "≓", "backsim" => "∽",
        "backsimeq" => "⋍", "subseteqq" => "⫅", "supseteqq" => "⫆",
        "sqsubset" => "⊏", "sqsupset" => "⊐", "sqsubseteq" => "⊑",
        "sqsupseteq" => "⊒", "nsubseteq" => "⊈", "nsupseteq" => "⊉",
        "nsubset" => "⊄", "nsupset" => "⊅", "nless" => "≮", "ngtr" => "≯",
        "nprec" => "⊀", "nsucc" => "⊁", "shortmid" => "∣",
        "shortparallel" => "∥", "nshortmid" => "∤", "nshortparallel" => "∦",
        "nparallel" => "∦", "nvdash" => "⊬", "nvDash" => "⊭", "nVdash" => "⊯",
        "nVDash" => "⊮", "because" => "∵", "therefore" => "∴",
        // 箭头
        "to" | "rightarrow" | "longrightarrow" => "→", "gets" | "leftarrow" => "←",
        "Rightarrow" | "implies" => "⇒", "Leftarrow" | "impliedby" => "⇐",
        "leftrightarrow" => "↔", "Leftrightarrow" | "iff" => "⇔",
        "mapsto" => "↦", "longmapsto" => "⟼", "uparrow" => "↑",
        "downarrow" => "↓", "updownarrow" => "↕", "Uparrow" => "⇑",
        "Downarrow" => "⇓", "Updownarrow" => "⇕", "nearrow" => "↗",
        "searrow" => "↘", "swarrow" => "↙", "nwarrow" => "↖",
        "rightharpoonup" => "⇀", "rightharpoondown" => "⇁",
        "leftrightharpoons" => "⇄", "rightleftharpoons" => "⇌",
        "hookrightarrow" => "↪", "hookleftarrow" => "↩",
        "leadsto" | "rightsquigarrow" => "⇝", "longleftrightarrow" => "⟷",
        "Longleftrightarrow" => "⟺", "longRightarrow" => "⟹",
        "longLeftarrow" => "⟸",
        // 其他
        "infty" => "∞", "partial" => "∂", "nabla" => "∇", "forall" => "∀",
        "exists" => "∃", "nexists" => "∄", "neg" | "lnot" => "¬",
        "emptyset" | "varnothing" => "∅", "aleph" => "ℵ", "hbar" | "hslash" => "ℏ",
        "ell" => "ℓ", "Re" => "ℜ", "Im" => "ℑ", "wp" => "℘", "prime" => "′",
        "angle" => "∠", "measuredangle" => "∡", "triangle" => "△",
        "square" => "□", "blacksquare" => "■", "blacktriangle" => "▲",
        "cdots" => "⋯", "ldots" | "dots" | "dotsc" => "…", "vdots" => "⋮",
        "ddots" => "⋱", "dotsb" | "dotsm" | "dotsi" => "⋯", "degree" => "°",
        "S" => "§", "P" => "¶", "checkmark" => "✓", "clubsuit" => "♣",
        "diamondsuit" => "♢", "heartsuit" => "♡", "spadesuit" => "♠",
        "flat" => "♭", "natural" => "♮", "sharp" => "♯", "top" => "⊤",
        "bot" => "⊥", "lceil" => "⌈", "rceil" => "⌉", "lfloor" => "⌊",
        "rfloor" => "⌋", "langle" => "⟨", "rangle" => "⟩", "Box" => "□",
        "Vert" => "‖", "vert" => "|", "lvert" => "|", "rvert" => "|",
        "lVert" => "‖", "rVert" => "‖", "surd" => "√", "backslash" => "\\",

        _ => return None,
    };
    Some(s)
}

fn blackboard(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A' => '𝔸', 'B' => '𝔹', 'C' => 'ℂ', 'D' => '𝔻', 'E' => '𝔼',
            'F' => '𝔽', 'G' => '𝔾', 'H' => 'ℍ', 'I' => '𝕀', 'J' => '𝕁',
            'K' => '𝕂', 'L' => '𝕃', 'M' => '𝕄', 'N' => 'ℕ', 'O' => '𝕆',
            'P' => 'ℙ', 'Q' => 'ℚ', 'R' => 'ℝ', 'S' => '𝕊', 'T' => '𝕋',
            'U' => '𝕌', 'V' => '𝕍', 'W' => '𝕎', 'X' => '𝕏', 'Y' => '𝕐',
            'Z' => 'ℤ', 'a' => '𝕒', 'b' => '𝕓', 'c' => '𝕔', 'd' => '𝕕',
            'e' => '𝕖', 'f' => '𝕗', 'g' => '𝕘', 'h' => '𝕙', 'i' => '𝕚',
            'j' => '𝕛', 'k' => '𝕜', 'l' => '𝕝', 'm' => '𝕞', 'n' => '𝕟',
            'o' => '𝕠', 'p' => '𝕡', 'q' => '𝕢', 'r' => '𝕣', 's' => '𝕤',
            't' => '𝕥', 'u' => '𝕦', 'v' => '𝕧', 'w' => '𝕨', 'x' => '𝕩',
            'y' => '𝕪', 'z' => '𝕫', '0' => '𝟘', '1' => '𝟙', '2' => '𝟚',
            '3' => '𝟛', '4' => '𝟜', '5' => '𝟝', '6' => '𝟞', '7' => '𝟟',
            '8' => '𝟠', '9' => '𝟡',
            _ => c,
        })
        .collect()
}

fn script(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A' => '𝒜', 'B' => 'ℬ', 'C' => '𝒞', 'D' => '𝒟', 'E' => 'ℰ',
            'F' => 'ℱ', 'G' => '𝒢', 'H' => 'ℋ', 'I' => 'ℐ', 'J' => '𝒥',
            'K' => '𝒦', 'L' => 'ℒ', 'M' => 'ℳ', 'N' => '𝒩', 'O' => '𝒪',
            'P' => '𝒫', 'Q' => '𝒬', 'R' => 'ℛ', 'S' => '𝒮', 'T' => '𝒯',
            'U' => '𝒰', 'V' => '𝒱', 'W' => '𝒲', 'X' => '𝒳', 'Y' => '𝒴',
            'Z' => '𝒵', 'a' => '𝒶', 'b' => '𝒷', 'c' => '𝒸', 'd' => '𝒹',
            'e' => 'ℯ', 'f' => '𝒻', 'g' => 'ℊ', 'h' => '𝒽', 'i' => '𝒾',
            'j' => '𝒿', 'k' => '𝓀', 'l' => '𝓁', 'm' => '𝓂', 'n' => '𝓃',
            'o' => 'ℴ', 'p' => '𝓅', 'q' => '𝓆', 'r' => '𝓇', 's' => '𝓈',
            't' => '𝓉', 'u' => '𝓊', 'v' => '𝓋', 'w' => '𝓌', 'x' => '𝓍',
            'y' => '𝓎', 'z' => '𝓏',
            _ => c,
        })
        .collect()
}

fn fraktur(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A' => '𝔄', 'B' => '𝔅', 'C' => 'ℭ', 'D' => '𝔇', 'E' => '𝔈',
            'F' => '𝔉', 'G' => '𝔊', 'H' => 'ℌ', 'I' => 'ℑ', 'J' => '𝔍',
            'K' => '𝔎', 'L' => '𝔏', 'M' => '𝔐', 'N' => '𝔑', 'O' => '𝔒',
            'P' => '𝔓', 'Q' => '𝔔', 'R' => 'ℜ', 'S' => '𝔖', 'T' => '𝔗',
            'U' => '𝔘', 'V' => '𝔙', 'W' => '𝔚', 'X' => '𝔛', 'Y' => '𝔜',
            'Z' => 'ℨ', 'a' => '𝔞', 'b' => '𝔟', 'c' => '𝔠', 'd' => '𝔡',
            'e' => '𝔢', 'f' => '𝔣', 'g' => '𝔤', 'h' => '𝔥', 'i' => '𝔦',
            'j' => '𝔧', 'k' => '𝔨', 'l' => '𝔩', 'm' => '𝔪', 'n' => '𝔫',
            'o' => '𝔬', 'p' => '𝔭', 'q' => '𝔮', 'r' => '𝔯', 's' => '𝔰',
            't' => '𝔱', 'u' => '𝔲', 'v' => '𝔳', 'w' => '𝔴', 'x' => '𝔵',
            'y' => '𝔶', 'z' => '𝔷',
            _ => c,
        })
        .collect()
}

// ---------------------------------------------------------------- 生成 OMML

struct Gen<'a> {
    font: &'a str,
}

impl Gen<'_> {
    fn run(&self, s: &str, upright: bool) -> String {
        let ital = if upright { "" } else { "<w:i/>" };
        let f = self.font;
        // m:r 的规范内容是 `m:rPr?, w:rPr?, m:t`：Word 运行属性 w:rPr 必须是
        // m:r 的**直接子元素**。包进 <m:ctrlPr> 里 Word 会直接判文档损坏
        // （m:ctrlPr 只用于 m:f/m:nary 这类结构元素，不用于 m:r）。
        // 已用 Word COM 实测 6 个变体验证：只有这种写法能打开。
        let rpr = if upright { "<m:rPr><m:nor/></m:rPr>" } else { "" };
        format!(
            "<m:r>{rpr}<w:rPr><w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:eastAsia=\"{f}\"/>{ital}</w:rPr><m:t xml:space=\"preserve\">{t}</m:t></m:r>",
            t = esc(s)
        )
    }

    fn e(&self, n: &N) -> String {
        format!("<m:e>{}</m:e>", self.emit(n))
    }

    fn emit(&self, n: &N) -> String {
        match n {
            N::Seq(v) => v.iter().map(|x| self.emit(x)).collect::<String>(),
            N::Txt(s, up) => self.run(s, *up),
            N::Frac(a, b) => format!(
                "<m:f><m:num>{}</m:num><m:den>{}</m:den></m:f>",
                self.emit(a),
                self.emit(b)
            ),
            N::Sqrt(deg, body) => {
                let hide = if deg.is_some() { "0" } else { "1" };
                let d = match deg {
                    Some(x) => self.emit(x),
                    None => String::new(),
                };
                format!(
                    "<m:rad><m:radPr><m:degHide m:val=\"{hide}\"/></m:radPr><m:deg>{d}</m:deg>{}</m:rad>",
                    self.e(body)
                )
            }
            N::Sup(b, s) => format!(
                "<m:sSup>{}<m:sup>{}</m:sup></m:sSup>",
                self.e(b),
                self.emit(s)
            ),
            N::Sub(b, s) => format!(
                "<m:sSub>{}<m:sub>{}</m:sub></m:sSub>",
                self.e(b),
                self.emit(s)
            ),
            N::SubSup(b, s, p) => format!(
                "<m:sSubSup>{}<m:sub>{}</m:sub><m:sup>{}</m:sup></m:sSubSup>",
                self.e(b),
                self.emit(s),
                self.emit(p)
            ),
            N::Nary(op, sub, sup, body) => {
                let sh = if sub.is_some() { "0" } else { "1" };
                let ph = if sup.is_some() { "0" } else { "1" };
                let s = sub.as_ref().map(|x| self.emit(x)).unwrap_or_default();
                let p = sup.as_ref().map(|x| self.emit(x)).unwrap_or_default();
                format!(
                    "<m:nary><m:naryPr><m:chr m:val=\"{op}\"/><m:subHide m:val=\"{sh}\"/><m:supHide m:val=\"{ph}\"/></m:naryPr><m:sub>{s}</m:sub><m:sup>{p}</m:sup>{}</m:nary>",
                    self.e(body)
                )
            }
            N::Func(name, body) => format!(
                "<m:func><m:fName>{}</m:fName>{}</m:func>",
                self.run(name, true),
                self.e(body)
            ),
            N::Delim(beg, end, body) => format!(
                "<m:d><m:dPr><m:begChr m:val=\"{b}\"/><m:sepChr m:val=\"\"/><m:endChr m:val=\"{e}\"/><m:grow m:val=\"on\"/></m:dPr>{inner}</m:d>",
                b = esc(beg),
                e = esc(end),
                inner = self.e(body)
            ),
            N::Matrix(rows) => {
                let mut s = String::from("<m:m>");
                for r in rows {
                    s.push_str("<m:mr>");
                    for c in r {
                        s.push_str(&self.e(c));
                    }
                    s.push_str("</m:mr>");
                }
                s.push_str("</m:m>");
                s
            }
            N::EqArr(rows) => {
                let mut s = String::from("<m:eqArr>");
                for r in rows {
                    s.push_str("<m:e>");
                    for c in r {
                        s.push_str(&self.emit(c));
                    }
                    s.push_str("</m:e>");
                }
                s.push_str("</m:eqArr>");
                s
            }
            N::Acc(chr, base) => format!(
                "<m:acc><m:accPr><m:chr m:val=\"{c}\"/></m:accPr>{inner}</m:acc>",
                c = esc(chr),
                inner = self.e(base)
            ),
            N::Bar(over, base) => format!(
                "<m:bar><m:barPr><m:pos m:val=\"{p}\"/></m:barPr>{inner}</m:bar>",
                p = if *over { "top" } else { "bot" },
                inner = self.e(base)
            ),
        }
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            _ => o.push(c),
        }
    }
    o
}

/// 生成可直接放进 `document.xml` 的公式元素。
/// `display = true` 时输出块级 `<m:oMathPara>`，否则输出行内 `<m:oMath>`。
pub fn latex_to_omml(latex: &str, font: &str, display: bool) -> String {
    let toks = tokenize(latex);
    let p = P { t: toks, i: 0 };
    let inner = p.parse_all();
    let g = Gen { font };
    let body = g.emit(&inner);
    let body = if body.is_empty() { g.run("", false) } else { body };
    if display {
        format!("<m:oMathPara><m:oMath>{body}</m:oMath></m:oMathPara>")
    } else {
        format!("<m:oMath>{body}</m:oMath>")
    }
}
