/// JASSS (Journal of Artificial Societies and Social Simulation) の年度一覧．
/// Volume 1 = 1998, Volume N = 1997 + N.
/// 各 Volume は通常 4 issue（Volume 2 のみ 3 issue）．
pub fn available_years() -> Vec<u16> {
    (1998..=2026).collect()
}

/// 年度から Volume 番号を算出
pub fn year_to_volume(year: u16) -> u16 {
    year - 1997
}

/// 指定 Volume の Issue 番号一覧
pub fn issues_for_volume(volume: u16) -> Vec<u16> {
    match volume {
        2 => vec![1, 2, 3],
        _ => vec![1, 2, 3, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_years_range() {
        let years = available_years();
        assert_eq!(years.first(), Some(&1998));
        assert_eq!(years.last(), Some(&2026));
    }

    #[test]
    fn test_year_to_volume() {
        assert_eq!(year_to_volume(1998), 1);
        assert_eq!(year_to_volume(2026), 29);
    }

    #[test]
    fn test_issues_for_volume() {
        assert_eq!(issues_for_volume(1), vec![1, 2, 3, 4]);
        assert_eq!(issues_for_volume(2), vec![1, 2, 3]);
        assert_eq!(issues_for_volume(29), vec![1, 2, 3, 4]);
    }
}
