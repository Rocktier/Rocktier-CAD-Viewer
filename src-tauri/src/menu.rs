//! 家族标准应用菜单（见《Rocktier家族软件通用准则》菜单一章）。
//!
//! 结构对齐常用桌面软件：应用 / 文件 / 编辑 / 显示 / 窗口 / 帮助。
//! 自定义项点击经 `on_menu_event` 转成 `menu-action` 事件发给前端；
//! 预定义项（撤销、拷贝、最小化、退出等）由系统本地化并自带快捷键。

use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};

pub const WEBSITE: &str = "https://rocktier.com/";
pub const FEEDBACK: &str = "mailto:hello@rocktier.com";

/// Menu labels for one language.
///
/// Same approach as PDF's `menu.rs`: a struct per language instead of widening
/// the old `l(zh, en)` closure to eight arguments — with eight positional
/// string arguments, swapping `ja` and `ko` compiles cleanly and silently shows
/// the wrong language. One field per call site makes that a compile error.
///
/// Unknown codes fall back to English rather than panicking, so a stale
/// `localStorage` value degrades to a usable menu.
struct MenuStrings {
    open: &'static str,
    file: &'static str,
    edit: &'static str,
    view: &'static str,
    window: &'static str,
    help: &'static str,
    fit_content: &'static str,
    fit_window: &'static str,
    layers_panel: &'static str,
    toggle_theme: &'static str,
    website: &'static str,
    feedback: &'static str,
    about: &'static str,
    license: &'static str,
}

impl MenuStrings {
    fn for_lang(lang: &str) -> Self {
        // Primary subtag, so "zh-CN" and "zh-Hans" both land on zh.
        let code = lang.split(['-', '_']).next().unwrap_or("");
        match code {
            "zh" => Self {
                open: "打开…", file: "文件", edit: "编辑", view: "显示", window: "窗口",
                help: "帮助", fit_content: "适应内容（忽略散点）", fit_window: "适应窗口",
                layers_panel: "图层面板", toggle_theme: "切换深浅主题", website: "官方网站",
                feedback: "反馈", about: "关于 Rocktier CAD Viewer", license: "许可与激活…",
            },
            "ja" => Self {
                open: "開く…", file: "ファイル", edit: "編集", view: "表示", window: "ウインドウ",
                help: "ヘルプ", fit_content: "内容に合わせる（散点は無視）", fit_window: "ウィンドウに合わせる",
                layers_panel: "レイヤーパネル", toggle_theme: "テーマを切り替え", website: "公式サイト",
                feedback: "フィードバック", about: "Rocktier CAD Viewer について", license: "ライセンス…",
            },
            "ko" => Self {
                open: "열기…", file: "파일", edit: "편집", view: "보기", window: "창",
                help: "도움말", fit_content: "내용에 맞춤(분포점 무시)", fit_window: "창에 맞춤",
                layers_panel: "레이어 패널", toggle_theme: "테마 전환", website: "공식 웹사이트",
                feedback: "피드백", about: "Rocktier CAD Viewer 정보", license: "라이선스…",
            },
            "de" => Self {
                open: "Öffnen…", file: "Datei", edit: "Bearbeiten", view: "Ansicht", window: "Fenster",
                help: "Hilfe", fit_content: "Inhalt einpassen (Ausreißer werden ignoriert)", fit_window: "Fenster einpassen",
                layers_panel: "Ebenenbereich", toggle_theme: "Design wechseln", website: "Website",
                feedback: "Feedback", about: "Über Rocktier CAD Viewer", license: "Lizenz…",
            },
            "es" => Self {
                open: "Abrir…", file: "Archivo", edit: "Editar", view: "Ver", window: "Ventana",
                help: "Ayuda", fit_content: "Ajustar al contenido (se ignoran los valores atípicos)", fit_window: "Ajustar a la ventana",
                layers_panel: "Panel de capas", toggle_theme: "Cambiar tema", website: "Sitio web",
                feedback: "Comentarios", about: "Acerca de Rocktier CAD Viewer", license: "Licencia…",
            },
            "pt" => Self {
                open: "Abrir…", file: "Arquivo", edit: "Editar", view: "Exibir", window: "Janela",
                help: "Ajuda", fit_content: "Ajustar ao conteúdo (valores discrepantes ignorados)", fit_window: "Ajustar à janela",
                layers_panel: "Painel de camadas", toggle_theme: "Alternar tema", website: "Site",
                feedback: "Comentários", about: "Sobre o Rocktier CAD Viewer", license: "Licença…",
            },
            "ar" => Self {
                open: "فتح…", file: "ملف", edit: "تحرير", view: "عرض", window: "نافذة",
                help: "مساعدة", fit_content: "ملاءمة المحتوى (تُتجاهل النقاط الشاذة)", fit_window: "ملاءمة النافذة",
                layers_panel: "لوحة الطبقات", toggle_theme: "تبديل المظهر", website: "الموقع",
                feedback: "ملاحظات", about: "حول Rocktier CAD Viewer", license: "الترخيص…",
            },
            // English is both the family default and the fallback.
            _ => Self {
                open: "Open…", file: "File", edit: "Edit", view: "View", window: "Window",
                help: "Help", fit_content: "Fit Content", fit_window: "Fit Window",
                layers_panel: "Layers Panel", toggle_theme: "Toggle Theme", website: "Website",
                feedback: "Feedback", about: "About Rocktier CAD Viewer", license: "License…",
            },
        }
    }
}

pub fn build(app: &tauri::AppHandle, lang: &str) -> tauri::Result<()> {
    let m = MenuStrings::for_lang(lang);

    let open_i = MenuItem::with_id(app, "open", m.open, true, Some("CmdOrCtrl+O"))?;
    let fit_i = MenuItem::with_id(app, "fit", m.fit_window, true, None::<&str>)?;
    let core_i = MenuItem::with_id(
        app,
        "fit-core",
        m.fit_content,
        true,
        None::<&str>,
    )?;
    let panel_i = MenuItem::with_id(app, "panel", m.layers_panel, true, None::<&str>)?;
    let theme_i = MenuItem::with_id(
        app,
        "theme",
        m.toggle_theme,
        true,
        None::<&str>,
    )?;
    let site_i = MenuItem::with_id(app, "website", m.website, true, None::<&str>)?;
    let mail_i = MenuItem::with_id(app, "feedback", m.feedback, true, None::<&str>)?;
    // 购买页上写着"打开应用 → License → 输入激活码"，所以应用里必须真有一个能到
    // 那儿的入口（授权胶囊在已激活/商店版下会隐藏，帮助菜单是常驻入口）。
    let license_i = MenuItem::with_id(app, "license", m.license, true, None::<&str>)?;

    let app_menu = Submenu::with_items(
        app,
        "Rocktier CAD Viewer",
        true,
        &[
            &PredefinedMenuItem::about(
                app,
                Some(m.about),
                                Some(AboutMetadata {
                    version: Some(env!("CARGO_PKG_VERSION").to_string()),
                    copyright: Some("Copyright 2026 Rocktier".to_string()),
                    ..Default::default()
                }),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;

    let file_menu = Submenu::with_items(
        app,
        m.file,
        true,
        &[
            &open_i,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        app,
        m.edit,
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;

    let view_menu = Submenu::with_items(
        app,
        m.view,
        true,
        &[
            &fit_i,
            &core_i,
            &PredefinedMenuItem::separator(app)?,
            &panel_i,
            &theme_i,
        ],
    )?;

    let window_menu = Submenu::with_items(
        app,
        m.window,
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;

    let help_menu = Submenu::with_items(
        app,
        m.help,
        true,
        &[&license_i, &site_i, &mail_i],
    )?;

    let menu = Menu::with_items(
        app,
        &[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu, &help_menu],
    )?;
    app.set_menu(menu)?;
    Ok(())
}
