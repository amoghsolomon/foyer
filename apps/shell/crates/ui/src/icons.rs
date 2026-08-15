use gpui::SharedString;
use gpui_component::IconNamed;

/// Lucide glyphs needed by Foyer Shell but not included in gpui-component's curated set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoyerShellIcon {
    Bluetooth,
    Display,
    Headphones,
    Lock,
    LogOut,
    Microphone,
    Notes,
    Contacts,
    Bookmarks,
    Pause,
    Play,
    Power,
    Tasks,
    Restart,
    SkipBack,
    SkipForward,
    Volume,
    Wifi,
}

impl IconNamed for FoyerShellIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Bluetooth => "icons/bluetooth.svg",
            Self::Display => "icons/monitor.svg",
            Self::Headphones => "icons/headphones.svg",
            Self::Lock => "icons/lock-keyhole.svg",
            Self::LogOut => "icons/log-out.svg",
            Self::Microphone => "icons/mic.svg",
            Self::Notes => "icons/notebook.svg",
            Self::Contacts => "icons/contact.svg",
            Self::Bookmarks => "icons/bookmark.svg",
            Self::Pause => "icons/pause.svg",
            Self::Play => "icons/play.svg",
            Self::Power => "icons/power.svg",
            Self::Tasks => "icons/list-todo.svg",
            Self::Restart => "icons/rotate-ccw.svg",
            Self::SkipBack => "icons/skip-back.svg",
            Self::SkipForward => "icons/skip-forward.svg",
            Self::Volume => "icons/volume-2.svg",
            Self::Wifi => "icons/wifi.svg",
        }
        .into()
    }
}
