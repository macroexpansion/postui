//! Theme palette: colors used by every render.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    pub fg: Color,
    pub border: Color,
    pub header: Color,
    pub footer: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub accent: Color,
    pub muted: Color,
    pub error: Color,
    pub warn: Color,
    pub success: Color,
    pub table_header: Color,
    pub row_stripe: Color,
}

pub static DEFAULT: Theme = Theme {
    name: "default",
    bg: Color::Reset,
    fg: Color::Reset,
    border: Color::DarkGray,
    header: Color::Cyan,
    footer: Color::DarkGray,
    selection_bg: Color::Blue,
    selection_fg: Color::White,
    accent: Color::Yellow,
    muted: Color::DarkGray,
    error: Color::Red,
    warn: Color::Yellow,
    success: Color::Green,
    table_header: Color::Cyan,
    row_stripe: Color::Reset,
};

pub static DRACULA: Theme = Theme {
    name: "dracula",
    bg: Color::Rgb(40, 42, 54),
    fg: Color::Rgb(248, 248, 242),
    border: Color::Rgb(98, 114, 164),
    header: Color::Rgb(189, 147, 249),
    footer: Color::Rgb(98, 114, 164),
    selection_bg: Color::Rgb(68, 71, 90),
    selection_fg: Color::Rgb(248, 248, 242),
    accent: Color::Rgb(255, 121, 198),
    muted: Color::Rgb(98, 114, 164),
    error: Color::Rgb(255, 85, 85),
    warn: Color::Rgb(241, 250, 140),
    success: Color::Rgb(80, 250, 123),
    table_header: Color::Rgb(139, 233, 253),
    row_stripe: Color::Rgb(44, 46, 60),
};

pub static GRUVBOX_DARK: Theme = Theme {
    name: "gruvbox-dark",
    bg: Color::Rgb(40, 40, 40),
    fg: Color::Rgb(235, 219, 178),
    border: Color::Rgb(102, 92, 84),
    header: Color::Rgb(131, 165, 152),
    footer: Color::Rgb(102, 92, 84),
    selection_bg: Color::Rgb(60, 56, 54),
    selection_fg: Color::Rgb(251, 241, 199),
    accent: Color::Rgb(254, 128, 25),
    muted: Color::Rgb(146, 131, 116),
    error: Color::Rgb(204, 36, 29),
    warn: Color::Rgb(215, 153, 33),
    success: Color::Rgb(152, 151, 26),
    table_header: Color::Rgb(131, 165, 152),
    row_stripe: Color::Rgb(50, 48, 47),
};

pub static NORD: Theme = Theme {
    name: "nord",
    bg: Color::Rgb(46, 52, 64),
    fg: Color::Rgb(216, 222, 233),
    border: Color::Rgb(76, 86, 106),
    header: Color::Rgb(136, 192, 208),
    footer: Color::Rgb(76, 86, 106),
    selection_bg: Color::Rgb(67, 76, 94),
    selection_fg: Color::Rgb(236, 239, 244),
    accent: Color::Rgb(180, 142, 173),
    muted: Color::Rgb(76, 86, 106),
    error: Color::Rgb(191, 97, 106),
    warn: Color::Rgb(235, 203, 139),
    success: Color::Rgb(163, 190, 140),
    table_header: Color::Rgb(143, 188, 187),
    row_stripe: Color::Rgb(59, 66, 82),
};

pub static SOLARIZED_DARK: Theme = Theme {
    name: "solarized-dark",
    bg: Color::Rgb(0, 43, 54),
    fg: Color::Rgb(131, 148, 150),
    border: Color::Rgb(88, 110, 117),
    header: Color::Rgb(38, 139, 210),
    footer: Color::Rgb(88, 110, 117),
    selection_bg: Color::Rgb(7, 54, 66),
    selection_fg: Color::Rgb(147, 161, 161),
    accent: Color::Rgb(181, 137, 0),
    muted: Color::Rgb(88, 110, 117),
    error: Color::Rgb(220, 50, 47),
    warn: Color::Rgb(181, 137, 0),
    success: Color::Rgb(133, 153, 0),
    table_header: Color::Rgb(38, 139, 210),
    row_stripe: Color::Rgb(7, 54, 66),
};

pub static EVERFOREST: Theme = Theme {
    name: "everforest",
    bg: Color::Rgb(51, 60, 67),
    fg: Color::Rgb(211, 198, 170),
    border: Color::Rgb(133, 146, 137),
    header: Color::Rgb(167, 192, 128),
    footer: Color::Rgb(133, 146, 137),
    selection_bg: Color::Rgb(167, 192, 128),
    selection_fg: Color::Rgb(30, 35, 38),
    accent: Color::Rgb(230, 152, 117),
    muted: Color::Rgb(133, 146, 137),
    error: Color::Rgb(230, 126, 128),
    warn: Color::Rgb(219, 188, 127),
    success: Color::Rgb(167, 192, 128),
    table_header: Color::Rgb(131, 192, 146),
    row_stripe: Color::Rgb(58, 70, 76),
};

pub static ALL: &[&'static Theme] = &[
    &DEFAULT,
    &DRACULA,
    &GRUVBOX_DARK,
    &NORD,
    &SOLARIZED_DARK,
    &EVERFOREST,
];

pub fn by_name(name: &str) -> Option<&'static Theme> {
    ALL.iter().copied().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_name_finds_each_built_in() {
        for t in ALL {
            assert_eq!(by_name(t.name).map(|x| x.name), Some(t.name));
        }
    }

    #[test]
    fn by_name_returns_none_for_unknown() {
        assert!(by_name("nonexistent").is_none());
    }
}
