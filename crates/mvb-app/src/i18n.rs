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
    pub menu_open: &'static str,
    pub menu_quit: &'static str,
    pub language: &'static str,

    pub no_build_loaded: &'static str,
    pub open_button: &'static str,
    pub drop_hint: &'static str,
    pub file_filter: &'static str,

    pub error_title: &'static str,
    pub error_unsupported: &'static str,
    pub error_read: &'static str,
    pub error_too_large: &'static str,
    pub ok: &'static str,

    pub author: &'static str,
    pub size: &'static str,
    pub blocks: &'static str,
    pub regions: &'static str,
    pub region_fallback: &'static str,

    pub layer: &'static str,
    pub layer_up: &'static str,
    pub layer_down: &'static str,

    pub view: &'static str,
    pub zoom_in: &'static str,
    pub zoom_out: &'static str,
    pub rotate_left: &'static str,
    pub rotate_right: &'static str,
    pub reset_view: &'static str,
    pub show_below: &'static str,
    pub show_grid: &'static str,

    pub status_hint: &'static str,

    pub view_2d: &'static str,
    pub view_3d: &'static str,
    pub visible_layers: &'static str,
    pub all_layers: &'static str,
    pub no_layers: &'static str,
    pub invert_layers: &'static str,
    pub status_hint_3d: &'static str,
    pub error_no_gpu: &'static str,

    pub menu_view: &'static str,
    pub show_materials: &'static str,
    pub materials: &'static str,
    pub scope_layer: &'static str,
    pub scope_total: &'static str,
    pub items: &'static str,
    pub kinds: &'static str,
    pub copy_list: &'static str,
    pub column_block: &'static str,
    pub column_count: &'static str,
    pub column_stacks: &'static str,
    pub no_materials: &'static str,

    pub menu_save_project: &'static str,
    pub menu_export: &'static str,
    pub format_project: &'static str,
    pub format_litematica: &'static str,
    pub format_sponge: &'static str,
    pub format_structure: &'static str,
    pub export_warning_title: &'static str,
    pub export_anyway: &'static str,
    pub cancel: &'static str,
    /// `{n}`: number of regions.
    pub loss_regions: &'static str,
    pub loss_litematica_data: &'static str,
    pub loss_worldedit_data: &'static str,
    pub loss_metadata: &'static str,
    /// `{n}`: largest size a structure block can load.
    pub loss_structure_limit: &'static str,
    pub saved: &'static str,
    pub error_save_title: &'static str,
}

const EN: Texts = Texts {
    menu_file: "File",
    menu_open: "Open…",
    menu_quit: "Quit",
    language: "Language",

    no_build_loaded: "No build loaded.",
    open_button: "Open a file…",
    drop_hint: "…or drop a file onto this window.",
    file_filter: "Builds",

    error_title: "Cannot open the file",
    error_unsupported: "Unsupported file type. Supported files: .litematic, .schem, .nbt, .mvb.",
    error_read: "The file could not be read:",
    error_too_large: "This build is too wide to be displayed.",
    ok: "OK",

    author: "Author",
    size: "Size",
    blocks: "Blocks",
    regions: "Regions",
    region_fallback: "Region",

    layer: "Layer",
    layer_up: "Layer above (⬆)",
    layer_down: "Layer below (⬇)",

    view: "View",
    zoom_in: "Zoom in",
    zoom_out: "Zoom out",
    rotate_left: "Rotate 90° left",
    rotate_right: "Rotate 90° right",
    reset_view: "Reset view",
    show_below: "Show the layer below",
    show_grid: "Grid",

    status_hint: "Wheel: zoom · Drag: move · ⬆ / ⬇: change layer",

    view_2d: "2D layer",
    view_3d: "3D",
    visible_layers: "Visible layers",
    all_layers: "All",
    no_layers: "None",
    invert_layers: "Invert",
    status_hint_3d: "Left drag: orbit · Right drag: move · Wheel: zoom",
    error_no_gpu: "The 3D view is not available on this graphics card.",

    menu_view: "View",
    show_materials: "Materials list",
    materials: "Materials",
    scope_layer: "This layer",
    scope_total: "Whole build",
    items: "items",
    kinds: "kinds",
    copy_list: "Copy the list",
    column_block: "Block",
    column_count: "Count",
    column_stacks: "Stacks",
    no_materials: "Nothing to place here.",

    menu_save_project: "Save project…",
    menu_export: "Export as",
    format_project: "Project (.mvb)",
    format_litematica: "Litematica (.litematic)",
    format_sponge: "WorldEdit (.schem)",
    format_structure: "Structure block (.nbt)",
    export_warning_title: "Some information will not be kept",
    export_anyway: "Export anyway",
    cancel: "Cancel",
    loss_regions: "The {n} regions will be merged into one.",
    loss_litematica_data: "Data specific to Litematica will not be kept (preview image, dates, scheduled ticks).",
    loss_worldedit_data: "Data specific to WorldEdit will not be kept (paste offset, biomes, WorldEdit metadata).",
    loss_metadata: "This format stores neither the name nor the description.",
    loss_structure_limit: "Larger than {n} blocks: a structure block cannot load it in one piece (the /place command can).",
    saved: "Saved:",
    error_save_title: "Cannot save the file",
};

const FR: Texts = Texts {
    menu_file: "Fichier",
    menu_open: "Ouvrir…",
    menu_quit: "Quitter",
    language: "Langue",

    no_build_loaded: "Aucune construction chargée.",
    open_button: "Ouvrir un fichier…",
    drop_hint: "…ou glissez un fichier dans cette fenêtre.",
    file_filter: "Constructions",

    error_title: "Impossible d'ouvrir le fichier",
    error_unsupported: "Type de fichier non pris en charge. Fichiers acceptés : .litematic, .schem, .nbt, .mvb.",
    error_read: "Le fichier n'a pas pu être lu :",
    error_too_large: "Cette construction est trop large pour être affichée.",
    ok: "OK",

    author: "Auteur",
    size: "Dimensions",
    blocks: "Blocs",
    regions: "Régions",
    region_fallback: "Région",

    layer: "Couche",
    layer_up: "Couche du dessus (⬆)",
    layer_down: "Couche du dessous (⬇)",

    view: "Vue",
    zoom_in: "Zoomer",
    zoom_out: "Dézoomer",
    rotate_left: "Pivoter de 90° à gauche",
    rotate_right: "Pivoter de 90° à droite",
    reset_view: "Réinitialiser la vue",
    show_below: "Afficher la couche du dessous",
    show_grid: "Grille",

    status_hint: "Molette : zoom · Glisser : déplacer · ⬆ / ⬇ : changer de couche",

    view_2d: "Couche 2D",
    view_3d: "3D",
    visible_layers: "Couches affichées",
    all_layers: "Toutes",
    no_layers: "Aucune",
    invert_layers: "Inverser",
    status_hint_3d: "Clic gauche glissé : tourner · Clic droit glissé : déplacer · Molette : zoom",
    error_no_gpu: "La vue 3D n'est pas disponible avec cette carte graphique.",

    menu_view: "Affichage",
    show_materials: "Liste des matériaux",
    materials: "Matériaux",
    scope_layer: "Cette couche",
    scope_total: "Toute la construction",
    items: "objets",
    kinds: "types",
    copy_list: "Copier la liste",
    column_block: "Bloc",
    column_count: "Quantité",
    column_stacks: "Piles",
    no_materials: "Rien à poser ici.",

    menu_save_project: "Enregistrer le projet…",
    menu_export: "Exporter en",
    format_project: "Projet (.mvb)",
    format_litematica: "Litematica (.litematic)",
    format_sponge: "WorldEdit (.schem)",
    format_structure: "Bloc de structure (.nbt)",
    export_warning_title: "Certaines informations ne seront pas conservées",
    export_anyway: "Exporter quand même",
    cancel: "Annuler",
    loss_regions: "Les {n} régions seront fusionnées en une seule.",
    loss_litematica_data: "Les données propres à Litematica ne seront pas conservées (aperçu, dates, ticks programmés).",
    loss_worldedit_data: "Les données propres à WorldEdit ne seront pas conservées (décalage de collage, biomes, métadonnées WorldEdit).",
    loss_metadata: "Ce format n'enregistre ni le nom ni la description.",
    loss_structure_limit: "Plus de {n} blocs de côté : un bloc de structure ne peut pas la charger d'un seul coup (la commande /place le peut).",
    saved: "Enregistré :",
    error_save_title: "Impossible d'enregistrer le fichier",
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
