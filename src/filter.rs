//! Filter operators observed in SSMS ObjectExplorer.dll (FilterOperator sequence).
//! Evidence: Equal, NotEqual, StartsWith, Contains, NotContains, LessThan,
//! LessThanEqual, GreaterThan, GreaterThanEqual, Between, NotBetween.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Mirrors the FilterOperator token order recovered from ObjectExplorer.dll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOperator {
    Equal,
    NotEqual,
    StartsWith,
    Contains,
    NotContains,
    LessThan,
    LessThanEqual,
    GreaterThan,
    GreaterThanEqual,
    Between,
    NotBetween,
}

impl FilterOperator {
    pub const ALL: [FilterOperator; 11] = [
        Self::Equal,
        Self::NotEqual,
        Self::StartsWith,
        Self::Contains,
        Self::NotContains,
        Self::LessThan,
        Self::LessThanEqual,
        Self::GreaterThan,
        Self::GreaterThanEqual,
        Self::Between,
        Self::NotBetween,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "Equal",
            Self::NotEqual => "NotEqual",
            Self::StartsWith => "StartsWith",
            Self::Contains => "Contains",
            Self::NotContains => "NotContains",
            Self::LessThan => "LessThan",
            Self::LessThanEqual => "LessThanEqual",
            Self::GreaterThan => "GreaterThan",
            Self::GreaterThanEqual => "GreaterThanEqual",
            Self::Between => "Between",
            Self::NotBetween => "NotBetween",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|o| *o == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

impl fmt::Display for FilterOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One filter row — analogous to FilterItem / IFilterRow in Object Explorer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterItem {
    pub column: String,
    pub operator: FilterOperator,
    pub value: String,
    /// Second bound for Between / NotBetween.
    pub value2: String,
}

impl FilterItem {
    pub fn new(
        column: impl Into<String>,
        operator: FilterOperator,
        value: impl Into<String>,
    ) -> Self {
        Self {
            column: column.into(),
            operator,
            value: value.into(),
            value2: String::new(),
        }
    }

    pub fn matches_text(&self, candidate: &str) -> bool {
        let c = candidate.to_ascii_lowercase();
        let v = self.value.to_ascii_lowercase();
        let v2 = self.value2.to_ascii_lowercase();
        match self.operator {
            FilterOperator::Equal => c == v,
            FilterOperator::NotEqual => c != v,
            FilterOperator::StartsWith => c.starts_with(&v),
            FilterOperator::Contains => c.contains(&v),
            FilterOperator::NotContains => !c.contains(&v),
            FilterOperator::LessThan => c < v,
            FilterOperator::LessThanEqual => c <= v,
            FilterOperator::GreaterThan => c > v,
            FilterOperator::GreaterThanEqual => c >= v,
            FilterOperator::Between => c >= v && c <= v2,
            FilterOperator::NotBetween => c < v || c > v2,
        }
    }
}

/// SearchConditions-style AND of filter rows (SSMS FilterDialog OK path).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchConditions {
    pub items: Vec<FilterItem>,
}

impl SearchConditions {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() || self.items.iter().all(|i| i.value.is_empty())
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Apply filters to a property map (Name, Schema, Owner, …).
    pub fn matches(&self, props: &[(&str, &str)]) -> bool {
        if self.is_empty() {
            return true;
        }
        self.items.iter().all(|item| {
            if item.value.is_empty() {
                return true;
            }
            props
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&item.column))
                .map(|(_, val)| item.matches_text(val))
                .unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_all_has_eleven_ssms_tokens_in_order() {
        let names: Vec<_> = FilterOperator::ALL.iter().map(|o| o.as_str()).collect();
        assert_eq!(
            names,
            [
                "Equal",
                "NotEqual",
                "StartsWith",
                "Contains",
                "NotContains",
                "LessThan",
                "LessThanEqual",
                "GreaterThan",
                "GreaterThanEqual",
                "Between",
                "NotBetween",
            ]
        );
    }

    #[test]
    fn next_cycles_full_ring() {
        let mut op = FilterOperator::Equal;
        let mut seen = vec![op];
        for _ in 0..10 {
            op = op.next();
            seen.push(op);
        }
        assert_eq!(seen.len(), 11);
        assert_eq!(op.next(), FilterOperator::Equal);
        assert_eq!(seen[3], FilterOperator::Contains);
    }

    #[test]
    fn display_matches_as_str() {
        for op in FilterOperator::ALL {
            assert_eq!(format!("{op}"), op.as_str());
        }
    }

    #[test]
    fn equal_and_not_equal_case_insensitive() {
        let eq = FilterItem::new("Name", FilterOperator::Equal, "Foo");
        assert!(eq.matches_text("foo"));
        assert!(eq.matches_text("FOO"));
        assert!(!eq.matches_text("food"));
        let ne = FilterItem::new("Name", FilterOperator::NotEqual, "Foo");
        assert!(ne.matches_text("bar"));
        assert!(!ne.matches_text("FOO"));
    }

    #[test]
    fn starts_contains_not_contains() {
        let s = FilterItem::new("Name", FilterOperator::StartsWith, "asp");
        assert!(s.matches_text("AspNetUsers"));
        assert!(!s.matches_text("NetAsp"));
        let c = FilterItem::new("Name", FilterOperator::Contains, "user");
        assert!(c.matches_text("AspNetUsers"));
        let nc = FilterItem::new("Name", FilterOperator::NotContains, "user");
        assert!(!nc.matches_text("AspNetUsers"));
        assert!(nc.matches_text("Product"));
    }

    #[test]
    fn lexicographic_comparisons() {
        let lt = FilterItem::new("Name", FilterOperator::LessThan, "m");
        assert!(lt.matches_text("abc"));
        assert!(!lt.matches_text("xyz"));
        let lte = FilterItem::new("Name", FilterOperator::LessThanEqual, "m");
        assert!(lte.matches_text("m"));
        let gt = FilterItem::new("Name", FilterOperator::GreaterThan, "m");
        assert!(gt.matches_text("z"));
        let gte = FilterItem::new("Name", FilterOperator::GreaterThanEqual, "m");
        assert!(gte.matches_text("m"));
    }

    #[test]
    fn between_and_not_between() {
        let mut b = FilterItem::new("Name", FilterOperator::Between, "a");
        b.value2 = "c".into();
        assert!(b.matches_text("b"));
        assert!(b.matches_text("a"));
        assert!(b.matches_text("c"));
        assert!(!b.matches_text("d"));
        let mut nb = FilterItem::new("Name", FilterOperator::NotBetween, "a");
        nb.value2 = "c".into();
        assert!(nb.matches_text("d"));
        assert!(!nb.matches_text("b"));
    }

    #[test]
    fn search_conditions_empty_when_no_values() {
        let mut sc = SearchConditions::default();
        assert!(sc.is_empty());
        sc.items
            .push(FilterItem::new("Name", FilterOperator::Contains, ""));
        assert!(sc.is_empty());
        sc.items[0].value = "x".into();
        assert!(!sc.is_empty());
        sc.clear();
        assert!(sc.is_empty());
    }

    #[test]
    fn search_conditions_and_semantics() {
        let sc = SearchConditions {
            items: vec![
                FilterItem::new("Name", FilterOperator::Contains, "user"),
                FilterItem::new("Schema", FilterOperator::Equal, "dbo"),
            ],
        };
        assert!(sc.matches(&[("Name", "AspNetUsers"), ("Schema", "dbo")]));
        assert!(!sc.matches(&[("Name", "AspNetUsers"), ("Schema", "Sales")]));
        assert!(!sc.matches(&[("Name", "Product"), ("Schema", "dbo")]));
    }

    #[test]
    fn search_conditions_column_match_is_case_insensitive() {
        let sc = SearchConditions {
            items: vec![FilterItem::new("name", FilterOperator::Equal, "x")],
        };
        assert!(sc.matches(&[("NAME", "x")]));
    }

    #[test]
    fn missing_column_fails_non_empty_filter() {
        let sc = SearchConditions {
            items: vec![FilterItem::new("Owner", FilterOperator::Equal, "sa")],
        };
        assert!(!sc.matches(&[("Name", "t")]));
    }

    #[test]
    fn serde_roundtrip_filter_item() {
        let item = FilterItem::new("Schema", FilterOperator::StartsWith, "Hu");
        let json = serde_json::to_string(&item).unwrap();
        let back: FilterItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back.column, "Schema");
        assert_eq!(back.operator, FilterOperator::StartsWith);
        assert_eq!(back.value, "Hu");
    }
}
