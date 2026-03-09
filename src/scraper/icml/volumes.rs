/// ICML年度とPMLRボリューム番号のマッピング
pub fn year_to_volume(year: u16) -> Option<u16> {
    match year {
        2018 => Some(80),
        2019 => Some(97),
        2020 => Some(119),
        2021 => Some(139),
        2022 => Some(162),
        2023 => Some(202),
        2024 => Some(235),
        _ => None,
    }
}

/// 対応している年度の一覧を返す
pub fn available_years() -> Vec<u16> {
    vec![2018, 2019, 2020, 2021, 2022, 2023, 2024]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_year_to_volume_known_years() {
        assert_eq!(year_to_volume(2018), Some(80));
        assert_eq!(year_to_volume(2024), Some(235));
        assert_eq!(year_to_volume(2023), Some(202));
    }

    #[test]
    fn test_year_to_volume_unknown_year() {
        assert_eq!(year_to_volume(2017), None);
        assert_eq!(year_to_volume(2025), None);
        assert_eq!(year_to_volume(0), None);
    }

    #[test]
    fn test_available_years() {
        let years = available_years();
        assert_eq!(years.len(), 7);
        assert!(years.contains(&2018));
        assert!(years.contains(&2024));
        // Should be sorted
        assert_eq!(years, vec![2018, 2019, 2020, 2021, 2022, 2023, 2024]);
    }
}
