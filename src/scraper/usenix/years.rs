/// USENIX Security の利用可能な年を返す．
pub fn available_years() -> Vec<u16> {
    let mut years = vec![1993, 1995, 1996];
    years.extend(1998..=crate::conference::current_year());
    years
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_years_range() {
        let years = available_years();
        assert_eq!(years.first(), Some(&1993));
        assert_eq!(years.last(), Some(&crate::conference::current_year()));
        for missing in [1990, 1991, 1992, 1994, 1997] {
            assert!(!years.contains(&missing));
        }
        for present in [1993, 1995, 1996, 1998, 2013, 2014] {
            assert!(years.contains(&present));
        }
    }

    #[test]
    fn test_available_years_sorted() {
        let years = available_years();
        let mut sorted = years.clone();
        sorted.sort();
        assert_eq!(years, sorted);
    }
}
