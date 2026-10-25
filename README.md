# md2word — Markdown 批量转 Word 工具

把 Markdown 文档转换成 `.docx`，**保留文本格式、字体字号、标题层级、表格、公式和图片位置**。
带图形界面，支持单选 / 批量勾选 / 整个目录递归转换，也可以纯命令行跑。

- 语言：Rust（2024 edition）
- 界面：egui / eframe（双击 `md2word.exe` 即启动 GUI；带参数则走命令行）
- Markdown 解析：pulldown-cmark（开启表格 / 删除线 / 任务列表 / 脚注 / 上下标 / 数学公式 / 定义列表扩展）
- Word 生成：knit-md-docx-rs（docx-rs 的 OMML 分支）
- 公式：**自研 LaTeX → OMML 转换器**（不依赖任何外部公式服务）

---

## 1. 功能特性

### 1.1 保留什么

| 维度 | 保留方式 |
|---|---|
| 标题层级 | H1–H6 映射到 Word 内置 Heading 1–6 样式，可分别配置字号；带大纲级别（导航窗格可见）与"与下段同页" |
| 字体字号 | 正文 / 西文 / 代码 / 公式四套字体独立可配，正文默认宋体小四（12pt） |
| 行内格式 | 加粗、斜体、删除线、行内代码（等宽 + 底纹）、上标、下标 |
| 表格 | 表头加粗 + 浅灰底纹、单元格对齐（左/中/右）、列宽按版心均分、可配置边框 |
| 公式 | 行内 `$...$` 与块级 `$$...$$` 全部转成 **Word 原生公式对象**（OMML），可在 Word 里继续编辑 |
| 图片 | 行内图与独占段图片两种形态，统一转 PNG 内嵌，按页宽等比缩放，独占段默认居中；文件缺失时给出占位提示而不中断 |
| 列表 | 多级有序 / 无序 / 任务列表（`- [x]`），使用 Word 原生编号，缩进悬挂正确 |
| 引用 | 多级嵌套，左边框 + 灰色文字 |
| 代码块 | 保留语言标注（等宽字体 + 底纹，不着色） |
| 链接 | 转为可点击超链接 |
| 分页 | 命令行合并多个文件时，文件之间自动插入分页符 |
| 中文细节 | 中英文混排换行处智能补空格；纯中文换行不插空格 |

### 1.2 批量能力

- GUI 里添加多个文件，**逐个勾选**要转换的（全选 / 全不选 / 反选）
- 添加文件夹时自动递归收集 `.md / .markdown / .mdown / .mkd`（可关递归）
- 输出方式二选一：与源文件同目录同名，或统一输出到指定目录
- 命令行支持把多个输入**合并成一个 docx**（`-o all.docx`）

---

## 2. 构建

```bash
# 调试构建（日常迭代用这个，快）
cargo build

# 发布构建
cargo build --release
```

产物：`target/debug/md2word.exe` 或 `target/release/md2word.exe`。

> **为什么 release 配置里关掉了 LTO？** `eframe` 的依赖树非常大，
> `lto = "thin" + codegen-units = 1` 会让单进程 rustc 串行优化整棵树，
> 实测链接阶段跑了 42 分钟都没结束；收益却很小。现在用默认并行 codegen，
> release 构建约 2 分钟完成。

运行环境要求（GUI 模式）：Windows 7+ / macOS / Linux 其一。非 Windows 平台
不需要额外步骤，程序会依次尝试 PingFang / Noto Sans CJK / 文泉驿等常见字体，
都找不到时界面中文会显示为方块，但转换功能不受影响。

---

## 3. 使用方式

### 3.1 图形界面（默认）

直接双击运行，不带任何参数：

```bash
./target/release/md2word.exe
```

界面分三块：

1. **顶部工具栏** — 添加文件… / 添加文件夹… / 清空列表 / 全选 / 全不选 / 反选，
   一个「递归子目录」开关。
2. **中间文件列表** — 每个文件一行：复选框、文件名、大小、转换状态（✓ / ✗ + 原因）、
   行尾「×」移除单项。
3. **底部** — 输出方式（同目录 / 指定目录）、格式设置面板（见 §4）、
   转换按钮和进度条。

所有设置（含输出目录、字体、字号）自动持久化，下次启动恢复。
转换在后台线程执行，界面不卡。

### 3.2 命令行

```bash
# 打开图形界面
md2word

# 转换多个文件，各自生成同名 .docx
md2word a.md b.md

# 多个输入合并成一个文档（自动分页分隔）
md2word a.md b.md -o all.docx

# 递归转换整个目录，输出到指定目录
md2word ./docs -d ./out

# 只转当前层，不进子目录
md2word ./docs --no-recursive

# 帮助
md2word -h
```

识别的输入扩展名：`.md`、`.markdown`、`.mdown`、`.mkd`（大小写不敏感）。
输入为目录时自动递归展开，转换前按路径排序，保证输出顺序稳定。

---

## 4. 输出样式默认值

GUI 的格式面板可改，默认值如下（均为常见中文文档规范）：

| 项 | 默认值 | 说明 |
|---|---|---|
| 正文中文字体 | 宋体 | |
| 正文西文字体 | Times New Roman | |
| 代码字体 | Consolas | |
| 公式字体 | Cambria Math | Word 公式标准字体 |
| 正文字号 | 24 半磅 = 12pt（小四） | |
| H1–H6 字号 | 36 / 32 / 28 / 26 / 24 / 24 | 即 18/16/14/13/12/12 pt |
| 标题颜色 | `#1F3864` | 深蓝 |
| 代码字号 | 20 半磅 = 10pt | |
| 引用颜色 | `#595959` | 灰 |
| 首行缩进 | 开 | 正文首行缩进 2 字符 |
| 行距 | 1.5 倍（360/240） | |
| 表格边框 | 开 | |
| 图片最大宽度 | 9070 缇 | A4 版心宽，超出等比缩放 |
| 图片居中 | 开 | 仅独占段落的图片 |

命令行模式使用同一套默认值（`Options::default()`），不读配置文件。

---

## 5. 公式：LaTeX → OMML

Word 里的公式不是文本，而是一棵 XML 结构树：分数是 `<m:f>`、根式是 `<m:rad>`、
求和是 `<m:nary>`、括号是 `<m:d>`、矩阵是 `<m:m>`。Rust 生态里没有现成的
LaTeX→OMML 库，所以这个转换器是**从零写的**：先递归下降解析成语法树，
再逐节点映射成 OMML 元素。

### 5.1 行内与块级

| 写法 | 输出形态 |
|---|---|
| `$E = mc^2$` | 行内公式，嵌在文字流里（`<w:r><m:oMath>`） |
| `$$ ... $$` | 块级公式，单独成段居中（`<w:p><m:oMathPara>`） |

### 5.2 支持的结构

- **上下标**：`x^2`、`x_1`、`x_i^2`（同时上下标）
- **分数**：`\frac{a}{b}`、`\dfrac`、`\tfrac`
- **根式**：`\sqrt{x}`、`\sqrt[n]{x}`
- **大运算符**（`<m:nary>`，自动带上下限）：`\sum` `\prod` `\int` `\iint`
  `\oint` `\bigcup` `\bigcap` `\bigoplus` 等
- **极限**：`\lim_{x \to 0}`（`<m:limLow>`）
- **括号**：`(a+b)`，以及 `\left( ... \right)` 自动伸缩括号，支持
  `\left\|` `\left\{` `\langle` 等
- **函数名**：`\sin` `\cos` `\log` `\ln` `\max` 等自动识别为正体（不斜体）
- **重音**：`\hat` `\vec` `\bar` `\tilde` `\dot` `\overline` `\underline`
- **矩阵环境**：`matrix` `pmatrix` `bmatrix` `smallmatrix` `array` `subarray`
- **分段/对齐环境**：`cases` `dcases` `rcases`、`aligned` `align` `align*`
  `alignedat` `split` `gather` `gathered`
- **文字模式**：`\text{...}`、`\mathrm` `\mathbf` `\mathbb` `\mathcal`
  `\mathfrak` `\mathit`
- **符号表**：希腊字母（`\alpha` `\beta` `\Gamma`…）、运算符（`\pm` `\cdot`
  `\times` `\div`…）、关系符（`\le` `\ge` `\ne` `\approx` `\equiv`…）、
  箭头（`\to` `\Rightarrow` `\leftarrow`…）、集合（`\in` `\subset` `\cup`
  `\emptyset` `\infty`…）

**降级策略**：遇到不认识的宏，不会丢弃，而是当正体字面量输出——
公式内容永远不丢，最多是样式不完美。

---

## 6. 项目结构

```
src/
├── main.rs     入口：无参数=GUI，带参数=CLI；系统字体探测（GUI 中文显示）
├── app.rs      egui 界面：文件列表/勾选/设置/进度，后台线程转换
├── markdown.rs pulldown-cmark 事件流 → 中间文档模型
├── model.rs    中间模型：Block（块级）/ Inline（行内）两层，解析与写出解耦
├── omml.rs     LaTeX → OMML：tokenizer + 递归下降解析 + OMML 生成
├── docx.rs     中间模型 → .docx：样式/编号/表格/图片 + 公式占位符回填
└── order.rs    XML 后处理：按 ECMA-376 重排属性元素顺序（见 §8）
samples/        完整示例（测试文档.md / 边界测试.md + 配图）
tcase/          Word 实机回归套件（见 §7）
```

**数据流**：

```
.md 文本 ──pulldown-cmark──▶ 事件流 ──▶ 中间模型 ──▶ docx-rs ──▶ document.xml
                                                      │
                          LaTeX ──▶ omml.rs ──▶ OMML 字符串 ──┘（占位符回填）
                                                                      │
                                                          order.rs 规范化 ──▶ .docx
```

公式走"占位符 + 回填"而不是直接调 docx-rs 的数学 API，是为了绕开它元素枚举的
限制（比如矩阵、`aligned` 环境它没有），同时复用其稳定的包结构（样式、关系、
Content_Types）。

---

## 7. 测试与验证

### 7.1 单元测试

```bash
cargo test
```

7 个用例，覆盖 XML 顺序规范化的边界：`<w:pPr` vs `<w:pPrChange` 前缀碰撞、
自闭合标签、转义文本、深层嵌套、UTF-8 字符边界。

### 7.2 Word 实机回归（重要）

schema 校验通过 ≠ Word 能打开。本机装有 Word 16.0，回归套件通过 **Word COM
自动化真实打开每个产物**来判定，而不是只做 XML 检查：

```bash
# 1. 生成全部测试用例（m01–m24 公式 + t01–t12 特性）
python tcase/gen.py
python tcase/gen2.py   # 特性用例

# 2. 用 release 二进制全部转换
for f in tcase/m*.md tcase/t*.md; do ./target/release/md2word.exe "$f"; done

# 3. Word 逐个打开，输出每个文件的真实结果
powershell tcase/test2.ps1    # 24 个公式用例
powershell tcase/test4.ps1    # 12 个特性用例 + 完整文档
```

当前状态：**38/38 通过**（24 公式 + 12 特性 + 2 完整文档），
`doc.OMaths.Count` 确认公式被 Word 识别为公式对象而非纯文本。

### 7.3 支持矩阵（实机验证过）

公式用例覆盖：简单变量、上标、下标、上下标并列、分数、根式、求和、括号、
函数名、pmatrix 矩阵、块级公式、`\text`、`\mathbb`、希腊字母、`\bar`+`\hat`、
`\vec`+`\cdot`、极限、`\left( \frac \right)`、积分、二次公式（嵌套）、
`aligned`、`cases`、指数+求和嵌套组合、行内+块级混排。

特性用例覆盖：标题层级、表格、图片、行内公式、块级公式、列表、代码块、
引用、超链接、粗斜体行内代码、以及以上全部混在一篇的文档。

---

## 8. 实现要点与踩坑记录

这一节记的是**为什么代码长这样**，改相关代码前建议先读。

### 8.1 docx-rs 输出的属性元素顺序不合规（Word 报"文件已损坏"）

`knit-md-docx-rs` 的各个 `build_to` 按 Rust 字段声明顺序输出子元素，而
ECMA-376 规定这些是 `xsd:sequence`，顺序错就是 schema 非法：

| 元素 | 库的输出 | 规范要求 |
|---|---|---|
| `w:pPr` | `rPr` 在最前，`keepNext` 在最后 | `rPr` 在最后，`keepNext` 第 2 |
| `w:rPr` | `sz, szCs, color, b, rFonts` | `rFonts, b, color, sz, szCs` |
| `w:numPr` | `numId, ilvl` | `ilvl, numId` |

这些错误出现在**每一个段落、每一个 run** 上，是全文档系统性违规。
修复方式没有改上游库，而是 `order.rs` 在 `pack()` 前把所有属性元素按规范顺序
稳定重排，只触碰列出的 11 类属性元素，其余字节原样保留。

### 8.2 `m:r` 里的 `w:rPr` 不能被 `m:ctrlPr` 包住

`m:r` 的规范结构是 `m:rPr?, w:rPr?, m:t`——Word 运行属性必须是 `m:r` 的
**直接子元素**。`m:ctrlPr` 是给 `m:f`/`m:nary` 这类结构元素用的。
曾经把 `w:rPr` 塞进 `m:ctrlPr`，导致所有含行内公式的文档 Word 拒开
（复现方式：`$x$` 挂而 `$x^2$` 过，因为后者外面套了 `m:sSup`，
错误的 run 结构被藏起来了）。用 6 个结构变体让 Word 逐一判决后定位。

### 8.3 styles.xml 里不能出现两个同名 styleId

`Docx::new()` 默认已生成一个空 `Normal` 样式，自己再 `add_style` 一个
`Normal` 就会重名，Word 同样判损坏。改用 `Styles::new().default_fonts(...)`
设 `docDefaults` 全局默认字体。

### 8.4 OMML 子元素顺序（容易搞反，已核对 ECMA-376 权威定义）

- `sSup` = `sSupPr?, e, sup` —— **`e`（底数）在前、脚本在后**
- `sSubSup` = `sSubSupPr?, e, sub, sup`
- `nary` = `naryPr?, sub, sup, e` —— `e` 在**最后**
- `func` = `funcPr?, fName, e`
- 所有 `*Pr` 都是 `minOccurs=0`，可省略，不是必需子元素

### 8.5 其他

- 图片统一转 PNG 内嵌（`Pic::new` 内部走 image crate），按 `image_max_twips`
  等比缩放
- `pulldown-cmark` 会把 `$$...$$` 包在 `Paragraph` 事件里发出来，
  块级公式分支必须能穿透行内帧，否则会被行内逻辑吞掉
- 中英文混排补空格的判断标准：换行**两端都不是 CJK** 才补
- 写测试数据时小心 shell/Python 双重转义：`\alpha` 经 bash 双引号再进
  Python 字符串会被吃成 `\x07lpha`。`tcase/gen.py` 一律用原始字符串并自带
  控制字符自检

---

## 9. 已知限制

- 代码块不做语法高亮（只做等宽 + 底纹）
- 表格表头行不跨页重复（无 `tblHeader`）
- 命令行模式固定用默认样式，改样式请用 GUI
- 脚注 `[^1]` 渲染为上标编号文本，脚注定义块被跳过，未生成 Word 真脚注
- 原始 HTML 块降级为等宽文本呈现，不执行
- 公式支持常见论文子集，不是完整 LaTeX（见 §5.2 清单）
- 未做真机 Mac / Linux 验证（仅 Windows + Word 16.0 实测）
