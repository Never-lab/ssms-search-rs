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
    pub fn new(column: impl Into<String>, operator: FilterOperator, value: impl Into<String>) -> Self {
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
    fn contains_and_startswith() {
        let c = FilterItem::new("Name", FilterOperator::Contains, "user");
        assert!(c.matches_text("AspNetUsers"));
        let s = FilterItem::new("Name", FilterOperator::StartsWith, "asp");
        assert!(s.matches_text("AspNetUsers"));
    }
}
