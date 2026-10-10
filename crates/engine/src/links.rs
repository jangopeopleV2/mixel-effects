//! Community and project links (shown in Help ▸, the About dialog, the start screen and the
//! header's Discord button).

pub const APP_NAME: &str = "MIXEL Effects";
pub const APP_SLUG: &str = "effectcraft";
pub const DISCORD: &str = "https://discord.gg/artcraft";
pub const WEBSITE: &str = "https://getartcraft.com";
pub const APP_PAGE: &str = "https://github.com/jangopeopleV2/mixel-effects";
pub const GITHUB: &str = "https://github.com/jangopeopleV2/mixel-effects";
pub const ISSUES: &str = "https://github.com/jangopeopleV2/mixel-effects/issues";

/// Sibling apps of the ArtCraft family: (name, what it is, GitHub repo, app page).
pub const SIBLINGS: &[(&str, &str)] = &[
    ("MIXEL Photo", "photocraft"),
    ("MIXEL Vector", "vectorcraft"),
    ("MIXEL Film", "filmcraft"),
    ("MIXEL Light", "lightcraft"),
    ("MIXEL Print", "pdfcraft"),
    ("MIXEL Design", "designcraft"),
];

pub fn github_of(slug: &str) -> String {
    "https://github.com/jangopeopleV2/mixel-effects".to_string()
}
pub fn page_of(slug: &str) -> String {
    "https://github.com/jangopeopleV2/mixel-effects".to_string()
}
