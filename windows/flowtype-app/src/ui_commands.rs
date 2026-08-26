use super::Page;

pub(super) const ID_NAV_STATUS: usize = 100;
pub(super) const ID_NAV_PHONES: usize = 101;
pub(super) const ID_NAV_SETTINGS: usize = 102;
pub(super) const ID_PAIR: usize = 110;
pub(super) const ID_SAVE_SETTINGS: usize = 120;
pub(super) const ID_REPAIR: usize = 121;
pub(super) const ID_NAME: usize = 122;
pub(super) const ID_AUTO_START: usize = 123;
pub(super) const ID_SHOW_FLOATING: usize = 124;
pub(super) const ID_UNPAIR_BASE: usize = 3000;
pub(super) const ID_TRAY_OPEN: usize = 4001;
pub(super) const ID_TRAY_PAIR: usize = 4002;
pub(super) const ID_TRAY_EXIT: usize = 4003;

pub(super) enum UiCommand {
    Navigate(Page),
    Pair,
    SaveSettings,
    ToggleAutoStart,
    ToggleFloating,
    RepairInjector,
    Exit,
    Unpair(usize),
}

pub(super) fn command_for_id(id: usize) -> Option<UiCommand> {
    Some(match id {
        ID_NAV_STATUS | ID_TRAY_OPEN => UiCommand::Navigate(Page::Status),
        ID_NAV_PHONES => UiCommand::Navigate(Page::Phones),
        ID_NAV_SETTINGS => UiCommand::Navigate(Page::Settings),
        ID_PAIR | ID_TRAY_PAIR => UiCommand::Pair,
        ID_SAVE_SETTINGS => UiCommand::SaveSettings,
        ID_AUTO_START => UiCommand::ToggleAutoStart,
        ID_SHOW_FLOATING => UiCommand::ToggleFloating,
        ID_REPAIR => UiCommand::RepairInjector,
        ID_TRAY_EXIT => UiCommand::Exit,
        value if (ID_UNPAIR_BASE..ID_UNPAIR_BASE + 1000).contains(&value) => {
            UiCommand::Unpair(value - ID_UNPAIR_BASE)
        }
        _ => return None,
    })
}
