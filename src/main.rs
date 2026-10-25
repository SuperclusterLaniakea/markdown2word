//! md2word —— Markdown 批量转 Word
//!
//! 直接双击运行 = 图形界面；带参数运行 = 命令行批量转换。

mod app;
mod docx;
mod markdown;
mod model;
mod omml;
mod order;

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        return cli(&args);
    }

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1020.0, 780.0])
            .with_min_inner_size([760.0, 560.0])
            .with_title("Markdown → Word 批量转换"),
        ..Default::default()
    };
    eframe::run_native(
        "md2word",
        opts,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc)))
        }),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 让界面能显示中文：egui 内置字体不含 CJK 字形，必须从系统字体补一个。
fn setup_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &[
            "C:\\Windows\\Fonts\\simhei.ttf",
            "C:\\Windows\\Fonts\\msyh.ttc",
            "C:\\Windows\\Fonts\\simkai.ttf",
            "C:\\Windows\\Fonts\\simfang.ttf",
            "C:\\Windows\\Fonts\\simsun.ttc",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ]
    } else {
        &[
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/arphic/uming.ttc",
        ]
    };

    for p in candidates {
        let Ok(bytes) = std::fs::read(p) else { continue };
        let mut fonts = egui::FontDefinitions::default();
        let name = "cjk".to_owned();
        fonts.font_data.insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().insert(0, name.clone());
        }
        ctx.set_fonts(fonts);
        return;
    }
    // 没找到系统字体就保持默认（界面中文会显示成方块，但功能不受影响）
}

fn cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut dir: Option<PathBuf> = None;
    let mut recursive = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => { i += 1; out = args.get(i).map(PathBuf::from); }
            "-d" | "--dir" => { i += 1; dir = args.get(i).map(PathBuf::from); }
            "--no-recursive" => recursive = false,
            "-h" | "--help" => {
                println!(
                    "用法:\n  md2word                      打开图形界面\n  \
                     md2word a.md b.md             分别转成同名 .docx\n  \
                     md2word a.md b.md -o all.docx 合并成一个文档\n  \
                     md2word ./docs -d ./out       批量转换整个目录"
                );
                return Ok(());
            }
            other => inputs.push(PathBuf::from(other)),
        }
        i += 1;
    }
    if inputs.is_empty() {
        return Ok(());
    }

    // 展开目录
    let mut files: Vec<PathBuf> = Vec::new();
    for p in inputs {
        if p.is_dir() {
            let walk = walkdir::WalkDir::new(&p);
            let walk = if recursive { walk } else { walk.max_depth(1) };
            for e in walk.into_iter().filter_map(|e| e.ok()) {
                if e.file_type().is_file()
                    && e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| matches!(x.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown" | "mkd"))
                        .unwrap_or(false)
                {
                    files.push(e.path().to_path_buf());
                }
            }
        } else {
            files.push(p);
        }
    }
    files.sort();

    let opt = docx::Options::default();

    if let Some(o) = out {
        docx::convert_multi(&files, &o, &opt)?;
        println!("已生成 {}", o.display());
        return Ok(());
    }

    let mut ok = 0usize;
    for f in &files {
        let dst = match &dir {
            Some(d) => d.join(format!(
                "{}.docx",
                f.file_stem().and_then(|s| s.to_str()).unwrap_or("output")
            )),
            None => f.with_extension("docx"),
        };
        match docx::convert(f, &dst, &opt) {
            Ok(()) => { ok += 1; println!("✓ {}", dst.display()); }
            Err(e) => eprintln!("✗ {} : {e}", f.display()),
        }
    }
    println!("完成 {ok}/{}", files.len());
    Ok(())
}
