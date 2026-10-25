//! egui 界面：文件选择 / 批量勾选 / 格式配置 / 转换进度

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use egui::{RichText, ScrollArea, TextEdit};
use serde::{Deserialize, Serialize};

use crate::docx::{self, Options};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum OutMode {
    Same,
    Custom,
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    Pending,
    Done,
    Failed(String),
}

struct Item {
    path: PathBuf,
    selected: bool,
    size: u64,
    state: State,
}

enum Msg {
    Log(String),
    Item(usize, Result<(), String>),
    Finished,
}

#[derive(Serialize, Deserialize)]
struct Prefs {
    opt: Options,
    out_mode: OutMode,
    out_dir: String,
    merge: bool,
    merge_name: String,
    recursive: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            opt: Options::default(),
            out_mode: OutMode::Same,
            out_dir: String::new(),
            merge: false,
            merge_name: "合并文档.docx".into(),
            recursive: true,
        }
    }
}

pub struct App {
    items: Vec<Item>,
    prefs: Prefs,
    running: bool,
    done: usize,
    total: usize,
    logs: Vec<String>,
    rx: Option<mpsc::Receiver<Msg>>,
    last_dir: Option<PathBuf>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            prefs: Prefs::default(),
            running: false,
            done: 0,
            total: 0,
            logs: Vec::new(),
            rx: None,
            last_dir: None,
        }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut s = Self::default();
        if let Some(storage) = cc.storage {
            if let Some(raw) = storage.get_string("prefs") {
                if let Ok(p) = serde_json::from_str::<Prefs>(&raw) {
                    s.prefs = p;
                }
            }
        }
        s
    }

    fn log(&mut self, s: String) {
        self.logs.push(s);
        if self.logs.len() > 300 {
            self.logs.remove(0);
        }
    }

    fn add_files(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            if self.items.iter().any(|i| i.path == p) {
                continue;
            }
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            self.items.push(Item { path: p, selected: true, size, state: State::Pending });
        }
    }

    fn add_folder(&mut self, dir: PathBuf) {
        let mut found = Vec::new();
        let walk = walkdir::WalkDir::new(&dir);
        let walk = if self.prefs.recursive { walk } else { walk.max_depth(1) };
        for e in walk.into_iter().filter_map(|e| e.ok()) {
            if !e.file_type().is_file() {
                continue;
            }
            let p = e.path();
            if let Some(ext) = p.extension().and_then(|x| x.to_str()) {
                if matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown" | "mkd") {
                    found.push(p.to_path_buf());
                }
            }
        }
        found.sort();
        let n = found.len();
        self.add_files(found);
        self.log(format!("从 {} 找到 {n} 个 Markdown 文件", dir.display()));
    }

    fn out_path_for(&self, md: &Path) -> PathBuf {
        match self.prefs.out_mode {
            OutMode::Same => md.with_extension("docx"),
            OutMode::Custom => {
                let dir = PathBuf::from(&self.prefs.out_dir);
                let name = md
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output")
                    .to_string();
                if dir.as_os_str().is_empty() {
                    md.with_extension("docx")
                } else {
                    dir.join(format!("{name}.docx"))
                }
            }
        }
    }

    fn merge_out_path(&self) -> PathBuf {
        let name = if self.prefs.merge_name.trim().is_empty() {
            "合并文档.docx".to_string()
        } else {
            self.prefs.merge_name.trim().to_string()
        };
        let name = if name.to_ascii_lowercase().ends_with(".docx") { name } else { format!("{name}.docx") };
        match self.prefs.out_mode {
            OutMode::Same => self
                .items
                .iter()
                .find(|i| i.selected)
                .map(|i| i.path.parent().unwrap_or(Path::new(".")).join(&name))
                .unwrap_or_else(|| PathBuf::from(&name)),
            OutMode::Custom => {
                let dir = PathBuf::from(&self.prefs.out_dir);
                if dir.as_os_str().is_empty() { PathBuf::from(&name) } else { dir.join(&name) }
            }
        }
    }

    fn start(&mut self) {
        if self.running {
            return;
        }
        let idxs: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.selected)
            .map(|(n, _)| n)
            .collect();
        if idxs.is_empty() {
            self.log("没有选中任何文件".into());
            return;
        }
        for i in &mut self.items {
            i.state = State::Pending;
        }
        self.total = idxs.len();
        self.done = 0;
        self.running = true;
        self.logs.clear();

        let (tx, rx) = mpsc::channel::<Msg>();
        self.rx = Some(rx);

        if self.prefs.merge {
            let paths: Vec<PathBuf> = idxs.iter().map(|i| self.items[*i].path.clone()).collect();
            let out = self.merge_out_path();
            let opt = self.prefs.opt.clone();
            self.log(format!("合并转换 {} 个文件 -> {}", paths.len(), out.display()));
            thread::spawn(move || {
                let r = docx::convert_multi(&paths, &out, &opt);
                let msg = match &r {
                    Ok(()) => format!("已生成 {}", out.display()),
                    Err(e) => format!("失败：{e}"),
                };
                let _ = tx.send(Msg::Log(msg));
                let _ = tx.send(Msg::Item(usize::MAX, r.map_err(|e| e.to_string())));
                let _ = tx.send(Msg::Finished);
            });
        } else {
            let jobs: Vec<(usize, PathBuf, PathBuf)> = idxs
                .iter()
                .map(|i| {
                    let src = self.items[*i].path.clone();
                    let dst = self.out_path_for(&src);
                    (*i, src, dst)
                })
                .collect();
            let opt = self.prefs.opt.clone();
            self.log(format!("开始转换 {} 个文件", jobs.len()));
            thread::spawn(move || {
                for (idx, src, dst) in jobs {
                    let r = docx::convert(&src, &dst, &opt);
                    let msg = match &r {
                        Ok(()) => format!("✓ {} -> {}", src.display(), dst.display()),
                        Err(e) => format!("✗ {} : {e}", src.display()),
                    };
                    let _ = tx.send(Msg::Log(msg));
                    let _ = tx.send(Msg::Item(idx, r.map_err(|e| e.to_string())));
                }
                let _ = tx.send(Msg::Finished);
            });
        }
    }

    fn poll(&mut self) {
        let Some(rx) = self.rx.take() else { return };
        let mut finished = false;
        {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Log(s) => self.log(s),
                    Msg::Item(idx, res) => {
                        self.done += 1;
                        let st = match res {
                            Ok(()) => State::Done,
                            Err(e) => State::Failed(e),
                        };
                        if idx == usize::MAX {
                            for i in self.items.iter_mut().filter(|i| i.selected) {
                                i.state = st.clone();
                            }
                        } else if let Some(it) = self.items.get_mut(idx) {
                            it.state = st;
                        }
                    }
                    Msg::Finished => finished = true,
                }
            }
        }
        if finished {
            self.running = false;
        } else {
            self.rx = Some(rx);
        }
    }

    // ------------------------------------------------------------ 界面

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("添加文件…").clicked() {
                let mut d = rfd::FileDialog::new().add_filter("Markdown", &["md", "markdown", "mdown", "mkd"]);
                if let Some(dir) = &self.last_dir {
                    d = d.set_directory(dir);
                }
                if let Some(paths) = d.pick_files() {
                    if let Some(f) = paths.first().and_then(|p| p.parent()) {
                        self.last_dir = Some(f.to_path_buf());
                    }
                    self.add_files(paths);
                }
            }
            if ui.button("添加文件夹…").clicked() {
                let mut d = rfd::FileDialog::new();
                if let Some(dir) = &self.last_dir {
                    d = d.set_directory(dir);
                }
                if let Some(dir) = d.pick_folder() {
                    self.last_dir = Some(dir.clone());
                    self.add_folder(dir);
                }
            }
            if ui.button("清空列表").clicked() && !self.running {
                self.items.clear();
            }
            ui.separator();
            if ui.button("全选").clicked() {
                for i in &mut self.items {
                    i.selected = true;
                }
            }
            if ui.button("全不选").clicked() {
                for i in &mut self.items {
                    i.selected = false;
                }
            }
            if ui.button("反选").clicked() {
                for i in &mut self.items {
                    i.selected = !i.selected;
                }
            }
            ui.separator();
            let n = self.items.iter().filter(|i| i.selected).count();
            ui.label(format!("共 {} 个，已选 {} 个", self.items.len(), n));
            ui.checkbox(&mut self.prefs.recursive, "递归子目录");
        });
    }

    fn file_list(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical().id_salt("files").max_height(260.0).show(ui, |ui| {
            if self.items.is_empty() {
                ui.colored_label(egui::Color32::GRAY, "还没有添加文件，点上面的「添加文件」或「添加文件夹」。");
                return;
            }
            let mut remove: Option<usize> = None;
            egui::Grid::new("filegrid")
                .num_columns(4)
                .striped(true)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for (n, it) in self.items.iter_mut().enumerate() {
                        ui.checkbox(&mut it.selected, "");
                        let name = it
                            .path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("?")
                            .to_string();
                        let (mark, color) = match &it.state {
                            State::Pending => ("", egui::Color32::DARK_GRAY),
                            State::Done => ("✓ ", egui::Color32::from_rgb(0x1a, 0x7f, 0x37)),
                            State::Failed(_) => ("✗ ", egui::Color32::from_rgb(0xc0, 0x00, 0x00)),
                        };
                        let label = RichText::new(format!("{mark}{name}")).color(color);
                        let resp = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
                        if let State::Failed(e) = &it.state {
                            resp.clone().on_hover_text(e);
                        }
                        if resp.clicked() {
                            it.selected = !it.selected;
                        }
                        ui.label(format!("{} KB", it.size / 1024));
                        ui.label(
                            RichText::new(it.path.to_string_lossy().to_string())
                                .small()
                                .color(egui::Color32::GRAY),
                        )
                        .on_hover_text(it.path.to_string_lossy().to_string());
                        if ui.small_button("×").clicked() {
                            remove = Some(n);
                        }
                        ui.end_row();
                    }
                });
            if let Some(n) = remove {
                self.items.remove(n);
            }
        });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.prefs.opt;
        egui::Grid::new("settings")
            .num_columns(4)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("输出方式");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.prefs.out_mode, OutMode::Same, "与源文件同目录");
                    ui.radio_value(&mut self.prefs.out_mode, OutMode::Custom, "指定目录");
                });
                ui.end_row();

                if self.prefs.out_mode == OutMode::Custom {
                    ui.label("输出目录");
                    ui.horizontal(|ui| {
                        ui.add(
                            TextEdit::singleline(&mut self.prefs.out_dir)
                                .desired_width(340.0)
                                .hint_text("留空则用源文件目录"),
                        );
                        if ui.button("浏览…").clicked() {
                            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                self.prefs.out_dir = d.to_string_lossy().to_string();
                            }
                        }
                    });
                    ui.end_row();
                }

                ui.label("正文字体");
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut o.body_font).desired_width(110.0));
                    ui.label("西文");
                    ui.add(TextEdit::singleline(&mut o.body_font_en).desired_width(130.0));
                });
                ui.end_row();

                ui.label("正文字号");
                ui.horizontal(|ui| {
                    let mut pt = o.body_size as f32 / 2.0;
                    if ui.add(egui::DragValue::new(&mut pt).range(5.0..=72.0).suffix(" pt")).changed() {
                        o.body_size = (pt * 2.0).round() as usize;
                    }
                    ui.label("（小四 = 12pt）");
                });
                ui.end_row();

                ui.label("标题字号 H1–H6");
                ui.horizontal_wrapped(|ui| {
                    for i in 0..6 {
                        let mut pt = o.heading_sizes[i] as f32 / 2.0;
                        if ui
                            .add(egui::DragValue::new(&mut pt).range(5.0..=72.0).prefix(format!("H{} ", i + 1)))
                            .changed()
                        {
                            o.heading_sizes[i] = (pt * 2.0).round() as usize;
                        }
                    }
                });
                ui.end_row();

                ui.label("代码/公式字体");
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut o.mono_font).desired_width(110.0));
                    ui.label("公式");
                    ui.add(TextEdit::singleline(&mut o.math_font).desired_width(130.0));
                });
                ui.end_row();

                ui.label("行距");
                ui.horizontal(|ui| {
                    let mut x = o.line_spacing as f32 / 240.0;
                    if ui.add(egui::DragValue::new(&mut x).range(1.0..=3.0).speed(0.05).suffix(" 倍")).changed() {
                        o.line_spacing = (x * 240.0).round() as i32;
                    }
                    ui.checkbox(&mut o.first_line_indent, "首行缩进 2 字符");
                });
                ui.end_row();

                ui.label("图片最大宽度");
                ui.horizontal(|ui| {
                    let mut cm = o.image_max_twips as f32 / 567.0;
                    if ui.add(egui::DragValue::new(&mut cm).range(2.0..=16.0).speed(0.1).suffix(" cm")).changed() {
                        o.image_max_twips = (cm * 567.0).round() as u32;
                    }
                    ui.checkbox(&mut o.center_images, "图片居中");
                    ui.checkbox(&mut o.table_borders, "表格边框");
                });
                ui.end_row();
            });

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.prefs.merge, "合并为一个 Word 文档");
            if self.prefs.merge {
                ui.label("文件名");
                ui.add(TextEdit::singleline(&mut self.prefs.merge_name).desired_width(180.0));
            }
        });

        if ui.small_button("恢复默认格式").clicked() {
            self.prefs.opt = Options::default();
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Ok(raw) = serde_json::to_string(&self.prefs) {
            storage.set_string("prefs", raw);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        if self.running {
            // 转换在后台线程跑，这里让它持续重绘以刷新进度
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(6.0);
            ui.heading("Markdown → Word 批量转换");
            ui.label("保留：文本格式 / 字号层级 / 表格 / LaTeX 公式（原生 Word 公式对象）/ 图片位置");
            ui.add_space(4.0);
            self.toolbar(ui);
            ui.add_space(4.0);
        });

        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.add_space(4.0);
            self.file_list(ui);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let btn = if self.running { "转换中…" } else { "开始转换" };
                if ui.add_enabled(!self.running, egui::Button::new(btn).min_size([120.0, 34.0].into())).clicked() {
                    self.start();
                }
                if self.total > 0 {
                    ui.add(
                        egui::ProgressBar::new(self.done as f32 / self.total as f32)
                            .text(format!("{}/{}", self.done, self.total)),
                    );
                }
                if ui.button("打开输出目录").clicked() {
                    if let Some(p) = self.items.iter().find(|i| i.selected).map(|i| i.path.clone()) {
                        let dir = match self.prefs.out_mode {
                            OutMode::Same => p.parent().map(|x| x.to_path_buf()).unwrap_or_default(),
                            OutMode::Custom => PathBuf::from(&self.prefs.out_dir),
                        };
                        let _ = open::that(dir);
                    }
                }
            });
            ui.add_space(2.0);
            ui.collapsing("日志", |ui| {
                ScrollArea::vertical().id_salt("logs").max_height(150.0).show(ui, |ui| {
                    for l in &self.logs {
                        ui.label(l);
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(4.0);
            egui::CollapsingHeader::new("转换设置")
                .default_open(true)
                .id_salt("cfg")
                .show(ui, |ui| {
                    self.settings(ui);
                });
            ui.add_space(8.0);
            ui.separator();
            ui.label(
                RichText::new(
                    "说明：公式会以 Word 原生公式对象写入（可双击编辑）；图片按相对路径从 Markdown 所在目录查找；\
                     表格会随列数平均分配宽度。",
                )
                .small()
                .color(egui::Color32::GRAY),
            );
        });
    }
}

// 让 `open` 可用（没有就用系统命令兜底）
mod open {
    pub fn that(p: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let p = p.as_ref();
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer").arg(p).spawn()?;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(p).spawn()?;
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            std::process::Command::new("xdg-open").arg(p).spawn()?;
        }
        Ok(())
    }
}
