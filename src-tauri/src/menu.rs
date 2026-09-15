//! 家族标准应用菜单（见《Rocktier家族软件通用准则》菜单一章）。
//!
//! 结构对齐常用桌面软件：应用 / 文件 / 编辑 / 显示 / 窗口 / 帮助。
//! 自定义项点击经 `on_menu_event` 转成 `menu-action` 事件发给前端；
//! 预定义项（撤销、拷贝、最小化、退出等）由系统本地化并自带快捷键。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

pub const WEBSITE: &str = "https://rocktier.com/";
pub const FEEDBACK: &str = "mailto:hello@rocktier.com";

pub fn build(app: &tauri::AppHandle, lang: &str) -> tauri::Result<()> {
    let zh = lang.starts_with("zh");
    let l = |zhv: &'static str, en: &'static str| if zh { zhv } else { en };

    let open_i = MenuItem::with_id(app, "open", l("打开…", "Open…"), true, Some("CmdOrCtrl+O"))?;
    let fit_i = MenuItem::with_id(app, "fit", l("适应窗口", "Fit Window"), true, None::<&str>)?;
    let core_i = MenuItem::with_id(
        app,
        "fit-core",
        l("适应内容（忽略散点）", "Fit Content"),
        true,
        None::<&str>,
    )?;
    let panel_i = MenuItem::with_id(app, "panel", l("图层面板", "Layers Panel"), true, None::<&str>)?;
    let site_i = MenuItem::with_id(app, "website", l("官方网站", "Website"), true, None::<&str>)?;
    let mail_i = MenuItem::with_id(app, "feedback", l("反馈", "Feedback"), true, None::<&str>)?;

    let app_menu = Submenu::with_items(
        app,
        "Rocktier CAD Viewer",
        true,
        &[
            &PredefinedMenuItem::about(
                app,
                Some(l("关于 Rocktier CAD Viewer", "About Rocktier CAD Viewer")),
                None,
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
        l("文件", "File"),
        true,
        &[
            &open_i,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        app,
        l("编辑", "Edit"),
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
        l("显示", "View"),
        true,
        &[&fit_i, &core_i, &PredefinedMenuItem::separator(app)?, &panel_i],
    )?;

    let window_menu = Submenu::with_items(
        app,
        l("窗口", "Window"),
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;

    let help_menu = Submenu::with_items(
        app,
        l("帮助", "Help"),
        true,
        &[&site_i, &mail_i],
    )?;

    let menu = Menu::with_items(
        app,
        &[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu, &help_menu],
    )?;
    app.set_menu(menu)?;
    Ok(())
}
