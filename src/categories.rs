//! YouTube category map (A3): categoryId → display title.
//!
//! Ported from the MW Metadata `ytCategory.lookup` table
//! (mattwright324/youtube-metadata, MIT — js/translators.js). Data-only:
//! the 32 known video categories (1..44, non-contiguous).

/// The 32 known categories: (categoryId, display title).
pub const YT_CATEGORIES: [(&str, &str); 32] = [
    ("1", "Film & Animation"),
    ("2", "Autos & Vehicles"),
    ("10", "Music"),
    ("15", "Pets & Animals"),
    ("17", "Sports"),
    ("18", "Short Movies"),
    ("19", "Travel & Events"),
    ("20", "Gaming"),
    ("21", "Videoblogging"),
    ("22", "People & Blogs"),
    ("23", "Comedy"),
    ("24", "Entertainment"),
    ("25", "News & Politics"),
    ("26", "Howto & Style"),
    ("27", "Education"),
    ("28", "Science & Technology"),
    ("29", "Nonprofits & Activism"),
    ("30", "Movies"),
    ("31", "Anime/Animation"),
    ("32", "Action/Adventure"),
    ("33", "Classics"),
    ("34", "Comedy"),
    ("35", "Documentary"),
    ("36", "Drama"),
    ("37", "Family"),
    ("38", "Foreign"),
    ("39", "Horror"),
    ("40", "Sci-Fi/Fantasy"),
    ("41", "Thriller"),
    ("42", "Shorts"),
    ("43", "Shows"),
    ("44", "Trailers"),
];

/// `categoryId` → display title; `None` for unknown ids (callers render the
/// raw id).
pub fn category_name(id: &str) -> Option<&'static str> {
    YT_CATEGORIES
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, title)| *title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_name_lookup() {
        assert_eq!(YT_CATEGORIES.len(), 32);
        assert_eq!(category_name("1"), Some("Film & Animation"));
        assert_eq!(category_name("28"), Some("Science & Technology"));
        assert_eq!(category_name("42"), Some("Shorts"));
        assert_eq!(category_name("44"), Some("Trailers"));
        assert_eq!(category_name("23"), Some("Comedy"));
        // Unknown / absent → None (render raw id).
        assert_eq!(category_name("999"), None);
        assert_eq!(category_name(""), None);
    }
}
