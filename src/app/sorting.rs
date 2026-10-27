use crate::media::ImageEntry;

#[derive(Clone, Copy, Default)]
pub enum Column {
    Type,
    File,
    #[default]
    Date,
    Source,
    Rating,
}
impl Column {
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Type),
            1 => Some(Self::File),
            2 => Some(Self::Date),
            3 => Some(Self::Source),
            4 => Some(Self::Rating),
            _ => None,
        }
    }
    pub fn index(self) -> i32 {
        self as i32
    }
}

pub fn sort(entries: &mut [ImageEntry], column: Column, descending: bool) {
    // Cache case-folded strings once per item rather than allocating per comparison.
    match column {
        Column::Date => entries.sort_by(|a, b| {
            match (a.date, b.date) {
                (Some(a_date), Some(b_date)) => {
                    let order = a_date.cmp(&b_date);
                    (if descending { order.reverse() } else { order })
                        .then_with(|| a.path.cmp(&b.path))
                }
                // Keep unknown dates last in either direction.
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => a.path.cmp(&b.path),
            }
        }),
        _ => {
            let key = |entry: &ImageEntry| match column {
                Column::Type => entry.format.to_lowercase(),
                Column::File => entry
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase(),
                Column::Source => entry.source.to_lowercase(),
                Column::Rating => entry.metadata["Rating"]
                    .as_i64()
                    .unwrap_or(0)
                    .clamp(0, 5)
                    .to_string(),
                Column::Date => unreachable!(),
            };
            if descending {
                entries.sort_by_cached_key(|entry| {
                    (std::cmp::Reverse(key(entry)), entry.path.clone())
                });
            } else {
                entries.sort_by_cached_key(|entry| (key(entry), entry.path.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name: &str, format: &str, seconds: Option<i64>, source: &'static str) -> ImageEntry {
        ImageEntry {
            metadata: Default::default(),
            path: name.into(),
            format: format.into(),
            date: seconds.and_then(|v| chrono::DateTime::from_timestamp(v, 0)),
            source,
            orientation: 1,
            video: false,
        }
    }
    #[test]
    fn sorts_all_columns_in_both_directions_and_keeps_unknown_dates_last() {
        let original = vec![
            entry("z.jpg", "JPEG", Some(10), "EXIF capture date"),
            entry("A.png", "PNG", Some(20), "File timestamp"),
            entry("b.bmp", "BMP", None, "Unknown"),
        ];
        for (column, ascending, descending) in [
            (
                Column::Type,
                ["b.bmp", "z.jpg", "A.png"],
                ["A.png", "z.jpg", "b.bmp"],
            ),
            (
                Column::File,
                ["A.png", "b.bmp", "z.jpg"],
                ["z.jpg", "b.bmp", "A.png"],
            ),
            (
                Column::Date,
                ["z.jpg", "A.png", "b.bmp"],
                ["A.png", "z.jpg", "b.bmp"],
            ),
            (
                Column::Source,
                ["z.jpg", "A.png", "b.bmp"],
                ["b.bmp", "A.png", "z.jpg"],
            ),
        ] {
            for (reverse, expected) in [(false, ascending), (true, descending)] {
                let mut entries = original.clone();
                sort(&mut entries, column, reverse);
                assert_eq!(
                    entries
                        .iter()
                        .map(|e| e.path.to_str().unwrap())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
    }
}
