//! The archive's bibliography.

use crate::error::Result;
use crate::models::{Book, Output};

use super::archive;

pub async fn books(no_cache: bool, filter: Option<&str>) -> Result<()> {
    let archive = archive(no_cache)?;
    let index = archive.books().await?;
    Output::success(filter_books(index, filter)).print();
    Ok(())
}

/// No filter lists the whole bibliography. A filter matches case-insensitively
/// against the section, any author, or the title — a hit on any one counts.
fn filter_books(index: Vec<Book>, filter: Option<&str>) -> Vec<Book> {
    let Some(filter) = filter else {
        return index;
    };
    let needle = filter.to_ascii_lowercase();
    index
        .into_iter()
        .filter(|b| matches_filter(b, &needle))
        .collect()
}

fn matches_filter(book: &Book, needle: &str) -> bool {
    book.section
        .as_deref()
        .is_some_and(|s| s.to_ascii_lowercase().contains(needle))
        || book
            .authors
            .iter()
            .any(|a| a.to_ascii_lowercase().contains(needle))
        || book.title.to_ascii_lowercase().contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(title: &str, authors: &[&str], section: Option<&str>) -> Book {
        Book {
            id: "x".into(),
            path: "/folk/books/#x".into(),
            title: title.into(),
            authors: authors.iter().map(|s| s.to_string()).collect(),
            publisher: None,
            year: None,
            section: section.map(String::from),
            online_url: None,
            cover_url: None,
        }
    }

    #[test]
    fn a_filter_matching_the_section_returns_that_whole_section() {
        let index = vec![
            book("Songs of the West", &["Sabine Baring-Gould"], Some("Ballads and Songs")),
            book("English Folk Song and Dance", &["Cecil Sharp"], Some("Folk Song and Music")),
        ];
        let filtered = filter_books(index, Some("ballads and songs"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Songs of the West");
    }

    #[test]
    fn a_filter_matching_one_author_does_not_return_books_by_others() {
        let index = vec![
            book("Songs of the West", &["Sabine Baring-Gould"], None),
            book("English Folk Song and Dance", &["Cecil Sharp"], None),
        ];
        let filtered = filter_books(index, Some("sharp"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "English Folk Song and Dance");
    }

    #[test]
    fn a_filter_matching_nothing_yields_an_empty_list_not_an_error() {
        let index = vec![book("Songs of the West", &["Sabine Baring-Gould"], None)];
        assert!(filter_books(index, Some("nonexistent")).is_empty());
    }

    #[test]
    fn a_filter_matches_the_title_too() {
        let index = vec![book("Songs of the West", &["Sabine Baring-Gould"], None)];
        assert_eq!(filter_books(index, Some("west")).len(), 1);
    }

    #[test]
    fn no_filter_returns_everything() {
        let index = vec![
            book("Songs of the West", &["Sabine Baring-Gould"], None),
            book("English Folk Song and Dance", &["Cecil Sharp"], None),
        ];
        assert_eq!(filter_books(index, None).len(), 2);
    }
}
