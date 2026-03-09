/// NDSS の利用可能な年リストを返す（ハードコード）
pub fn available_years() -> Vec<u16> {
    (2014..=2025).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_years_range() {
        let years = available_years();
        assert!(years.contains(&2014));
        assert!(years.contains(&2025));
        assert!(!years.contains(&2013));
        assert!(!years.contains(&2026));
    }

    #[test]
    fn test_available_years_count() {
        let years = available_years();
        assert_eq!(years.len(), 12);
    }

    #[test]
    fn test_available_years_sorted() {
        let years = available_years();
        let mut sorted = years.clone();
        sorted.sort();
        assert_eq!(years, sorted);
    }
}
