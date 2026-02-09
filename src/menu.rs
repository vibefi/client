#[cfg(target_os = "macos")]
pub fn setup_macos_app_menu(app_name: &str) {
    use objc2::{MainThreadOnly, sel};
    use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
    use objc2_foundation::{MainThreadMarker, NSString};

    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    if app.mainMenu().is_some() {
        return;
    }

    let menubar = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(""));

    let app_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(app_name));
    let app_menu_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(""),
            None,
            &NSString::from_str(""),
        )
    };
    menubar.addItem(&app_menu_item);
    app_menu_item.setSubmenu(Some(&app_menu));

    let quit_title = format!("Quit {app_name}");
    let quit_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(&quit_title),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    app_menu.addItem(&quit_item);

    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));
    let edit_menu_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Edit"),
            None,
            &NSString::from_str(""),
        )
    };
    menubar.addItem(&edit_menu_item);
    edit_menu_item.setSubmenu(Some(&edit_menu));

    let undo_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Undo"),
            Some(sel!(undo:)),
            &NSString::from_str("z"),
        )
    };
    undo_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&undo_item);

    let redo_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Redo"),
            Some(sel!(redo:)),
            &NSString::from_str("Z"),
        )
    };
    redo_item
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
    edit_menu.addItem(&redo_item);

    edit_menu.addItem(&NSMenuItem::separatorItem(mtm));

    let cut_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Cut"),
            Some(sel!(cut:)),
            &NSString::from_str("x"),
        )
    };
    cut_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&cut_item);

    let copy_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Copy"),
            Some(sel!(copy:)),
            &NSString::from_str("c"),
        )
    };
    copy_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&copy_item);

    let paste_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Paste"),
            Some(sel!(paste:)),
            &NSString::from_str("v"),
        )
    };
    paste_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&paste_item);

    let select_all_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Select All"),
            Some(sel!(selectAll:)),
            &NSString::from_str("a"),
        )
    };
    select_all_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&select_all_item);

    app.setMainMenu(Some(&menubar));
}
