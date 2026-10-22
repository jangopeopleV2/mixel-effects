//! Community and project links (shown in Help ▸, the About dialog, the start screen and the
//! header's project buttons).

pub const APP_NAME: &str = "MIXEL Effects";
pub const APP_SLUG: &str = "effectcraft";
pub const WEBSITE: &str = "https://www.mixel.ch";
pub const APP_PAGE: &str = "https://github.com/jangopeopleV2/mixel-effects";
pub const GITHUB: &str = "https://github.com/jangopeopleV2/mixel-effects";
pub const ISSUES: &str = "https://github.com/jangopeopleV2/mixel-effects/issues";

/// Sibling apps of the MIXEL family: (name, legacy app slug).
pub const SIBLINGS: &[(&str, &str)] = &[
    ("MIXEL Photo", "photocraft"),
    ("MIXEL Vector", "vectorcraft"),
    ("MIXEL Film", "filmcraft"),
    ("MIXEL Light", "lightcraft"),
    ("MIXEL Print", "pdfcraft"),
    ("MIXEL Design", "designcraft"),
];

/// Known apps use their MIXEL forks. Unknown slugs keep upstream's GitHub path
/// semantics under its fixed HTTPS authority rather than selecting an unrelated fork.
pub fn github_of(slug: &str) -> String {
    match slug {
        "photocraft" => "https://github.com/jangopeopleV2/mixel-photo".to_string(),
        "vectorcraft" => "https://github.com/jangopeopleV2/mixel-vector".to_string(),
        "filmcraft" => "https://github.com/jangopeopleV2/mixel-film".to_string(),
        "lightcraft" => "https://github.com/jangopeopleV2/mixel-light".to_string(),
        "pdfcraft" => "https://github.com/jangopeopleV2/mixel-print".to_string(),
        "effectcraft" => "https://github.com/jangopeopleV2/mixel-effects".to_string(),
        "designcraft" => "https://github.com/jangopeopleV2/mixel-design".to_string(),
        _ => format!("https://github.com/storytold/{slug}"),
    }
}

/// Known apps share their fork's project page; unknown slugs retain upstream's
/// company website for unsupported app slugs.
pub fn page_of(slug: &str) -> String {
    match slug {
        "photocraft" => "https://github.com/jangopeopleV2/mixel-photo".to_string(),
        "vectorcraft" => "https://github.com/jangopeopleV2/mixel-vector".to_string(),
        "filmcraft" => "https://github.com/jangopeopleV2/mixel-film".to_string(),
        "lightcraft" => "https://github.com/jangopeopleV2/mixel-light".to_string(),
        "pdfcraft" => "https://github.com/jangopeopleV2/mixel-print".to_string(),
        "effectcraft" => "https://github.com/jangopeopleV2/mixel-effects".to_string(),
        "designcraft" => "https://github.com/jangopeopleV2/mixel-design".to_string(),
        _ => WEBSITE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sibling_routes_to_its_distinct_mixel_fork() {
        let expected = [
            ("photocraft", "https://github.com/jangopeopleV2/mixel-photo"),
            ("vectorcraft", "https://github.com/jangopeopleV2/mixel-vector"),
            ("filmcraft", "https://github.com/jangopeopleV2/mixel-film"),
            ("lightcraft", "https://github.com/jangopeopleV2/mixel-light"),
            ("pdfcraft", "https://github.com/jangopeopleV2/mixel-print"),
            ("designcraft", "https://github.com/jangopeopleV2/mixel-design"),
        ];
        assert_eq!(SIBLINGS.len(), expected.len());
        let mut destinations = std::collections::BTreeSet::new();
        for ((_, actual_slug), (slug, url)) in SIBLINGS.iter().zip(expected) {
            assert_eq!(*actual_slug, slug);
            assert_eq!(github_of(slug), url);
            assert_eq!(page_of(slug), url);
            assert_ne!(url, GITHUB);
            assert!(destinations.insert(url), "siblings must not share a destination");
        }
    }

    #[test]
    fn own_slug_and_unknown_fallback_keep_their_intended_destinations() {
        assert_eq!(github_of(APP_SLUG), GITHUB);
        assert_eq!(page_of(APP_SLUG), APP_PAGE);
        assert_eq!(github_of("futurecraft"), "https://github.com/storytold/futurecraft");
        assert_eq!(page_of("futurecraft"), WEBSITE);
        assert_eq!(github_of(""), "https://github.com/storytold/");
        assert_eq!(page_of(""), WEBSITE);
        assert_eq!(WEBSITE, "https://www.mixel.ch");
    }
}
