//! User interface translations.
//!
//! Every language is a `Texts` constant: a missing translation is a compile error, and looking up
//! a string costs nothing at runtime.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    French,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::French];

    /// Picks the language matching the operating system locale, English otherwise.
    pub fn detect() -> Self {
        sys_locale::get_locale()
            .map(|tag| Self::from_locale(&tag))
            .unwrap_or(Language::English)
    }

    /// Maps a locale tag such as `fr-FR`, `fr_CA` or `en-US` to a supported language.
    pub fn from_locale(tag: &str) -> Self {
        let primary = tag.split(['-', '_']).next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("fr") {
            Language::French
        } else {
            Language::English
        }
    }

    /// Name of the language written in that language, for the language selector.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::French => "Français",
        }
    }

    pub fn texts(self) -> &'static Texts {
        match self {
            Language::English => &EN,
            Language::French => &FR,
        }
    }
}

pub struct Texts {
    pub menu_file: &'static str,
    pub menu_quit: &'static str,
    pub language: &'static str,
    pub no_build_loaded: &'static str,
}

const EN: Texts = Texts {
    menu_file: "File",
    menu_quit: "Quit",
    language: "Language",
    no_build_loaded: "No build loaded.",
};

const FR: Texts = Texts {
    menu_file: "Fichier",
    menu_quit: "Quitter",
    language: "Langue",
    no_build_loaded: "Aucune construction chargée.",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_locales_select_french() {
        for tag in ["fr", "fr-FR", "fr_CA", "FR-be"] {
            assert_eq!(Language::from_locale(tag), Language::French, "{tag}");
        }
    }

    #[test]
    fn other_locales_fall_back_to_english() {
        for tag in ["en-US", "de-DE", "frr", "", "C"] {
            assert_eq!(Language::from_locale(tag), Language::English, "{tag}");
        }
    }

    #[test]
    fn native_names_are_distinct() {
        assert_ne!(
            Language::English.native_name(),
            Language::French.native_name()
        );
    }
}
