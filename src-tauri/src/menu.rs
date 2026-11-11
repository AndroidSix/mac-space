use tauri::{
    AppHandle, Manager,
    menu::{Menu, PredefinedMenuItem as Item, Submenu},
};

pub fn localize(app: &AppHandle, language: &str) -> tauri::Result<()> {
    let zh = language == "zh";
    let label = |chinese, english| if zh { chinese } else { english };
    let app_menu = Submenu::with_items(
        app,
        "MacSpace",
        true,
        &[
            &Item::about(app, Some(label("关于清衡", "About MacSpace")), None)?,
            &Item::separator(app)?,
            &Item::hide(app, Some(label("隐藏清衡", "Hide MacSpace")))?,
            &Item::hide_others(app, Some(label("隐藏其他应用", "Hide Others")))?,
            &Item::show_all(app, Some(label("显示全部", "Show All")))?,
            &Item::separator(app)?,
            &Item::quit(app, Some(label("退出清衡", "Quit MacSpace")))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        label("编辑", "Edit"),
        true,
        &[
            &Item::undo(app, Some(label("撤销", "Undo")))?,
            &Item::redo(app, Some(label("重做", "Redo")))?,
            &Item::separator(app)?,
            &Item::cut(app, Some(label("剪切", "Cut")))?,
            &Item::copy(app, Some(label("复制", "Copy")))?,
            &Item::paste(app, Some(label("粘贴", "Paste")))?,
            &Item::select_all(app, Some(label("全选", "Select All")))?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        label("窗口", "Window"),
        true,
        &[
            &Item::minimize(app, Some(label("最小化", "Minimize")))?,
            &Item::maximize(app, Some(label("缩放", "Zoom")))?,
            &Item::close_window(app, Some(label("关闭窗口", "Close Window")))?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&app_menu, &edit, &window])?)?;
    if let Some(window) = app.get_webview_window("main") {
        window.set_title(label("清衡 · MacSpace", "MacSpace"))?;
    }
    Ok(())
}
