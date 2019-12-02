use std::cmp::Ordering;

pub const CHANGELOGS: &[(&str, &str)] = &[
    ("19.10", include_str!("../changelogs/19.10")),
    ("19.04", include_str!("../changelogs/19.04")),
    ("18.10", include_str!("../changelogs/18.10")),
];

pub fn since<'a>(release: &'a str) -> impl Iterator<Item = (&'static str, &'static str)> + 'a {
    CHANGELOGS
        .iter()
        .filter(move |(version, _)| human_sort::compare(release, *version) == Ordering::Less)
        .map(|(version, changelog)| (*version, *changelog))
}
