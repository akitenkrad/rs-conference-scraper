/// NDSS の利用可能な年リストを返す．
pub fn available_years() -> Vec<u16> {
    (1997..=crate::conference::current_year()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_years_range() {
        let years = available_years();
        assert_eq!(years.first(), Some(&1997));
        assert!(years.contains(&1997));
        assert!(years.contains(&2014));
        assert!(years.contains(&2025));
        assert!(years.contains(&crate::conference::current_year()));
        assert!(!years.contains(&(crate::conference::current_year() + 1)));
    }

    #[test]
    fn test_available_years_count() {
        let years = available_years();
        assert_eq!(
            years.len(),
            (crate::conference::current_year() - 1997 + 1) as usize
        );
    }

    #[test]
    fn test_available_years_sorted() {
        let years = available_years();
        let mut sorted = years.clone();
        sorted.sort();
        assert_eq!(years, sorted);
    }
}
