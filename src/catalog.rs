//! Object catalog with SMO-style URN paths.
//! Observed templates in ObjectExplorer.dll include:
//!   Server[@Name='{0}']/Database[@Name='{1}']/{2}{3}
//!   Server[@Name='{0}']/Information

use crate::filter::SearchConditions;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectKind {
    Server,
    Database,
    Schema,
    Table,
    View,
    Procedure,
    Function,
    Folder,
    Column,
    Index,
    Synonym,
    Trigger,
}

impl ObjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::Database => "Database",
            Self::Schema => "Schema",
            Self::Table => "Table",
            Self::View => "View",
            Self::Procedure => "StoredProcedure",
            Self::Function => "UserDefinedFunction",
            Self::Folder => "Folder",
            Self::Column => "Column",
            Self::Index => "Index",
            Self::Synonym => "Synonym",
            Self::Trigger => "Trigger",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogObject {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub kind: ObjectKind,
    pub name: String,
    pub schema: String,
    pub owner: String,
    pub urn: String,
    /// ISO-ish create date (empty when unknown).
    #[serde(default)]
    pub create_date: String,
}

impl CatalogObject {
    pub fn props(&self) -> [(&str, &str); 5] {
        [
            ("Name", self.name.as_str()),
            ("Schema", self.schema.as_str()),
            ("Owner", self.owner.as_str()),
            ("Type", self.kind.as_str()),
            ("Date", self.create_date.as_str()),
        ]
    }

    pub fn matches(&self, conditions: &SearchConditions) -> bool {
        // Objects without a known date (columns, indexes, unloaded kinds) never match a Date filter.
        if self.create_date.is_empty()
            && conditions
                .items
                .iter()
                .any(|i| i.column.eq_ignore_ascii_case("Date") && !i.value.is_empty())
        {
            return false;
        }
        conditions.matches(&self.props())
    }
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub objects: Vec<CatalogObject>,
}

impl Catalog {
    /// Demo tree shaped like Object Explorer (no live SQL required).
    pub fn demo() -> Self {
        let mut objects = Vec::new();
        let mut next = 1u64;
        let mut add = |parent: Option<u64>,
                       kind: ObjectKind,
                       name: &str,
                       schema: &str,
                       owner: &str,
                       urn: String|
         -> u64 {
            let id = next;
            next += 1;
            objects.push(CatalogObject {
                id,
                parent_id: parent,
                kind,
                name: name.to_string(),
                schema: schema.to_string(),
                owner: owner.to_string(),
                urn,
                create_date: String::new(),
            });
            id
        };

        let server = add(
            None,
            ObjectKind::Server,
            "localhost",
            "",
            "sa",
            "Server[@Name='localhost']".into(),
        );
        let db = add(
            Some(server),
            ObjectKind::Database,
            "AdventureWorks",
            "",
            "dbo",
            "Server[@Name='localhost']/Database[@Name='AdventureWorks']".into(),
        );
        let tables = add(
            Some(db),
            ObjectKind::Folder,
            "Tables",
            "",
            "",
            "Server[@Name='localhost']/Database[@Name='AdventureWorks']/Table".into(),
        );
        let mut employee_id = None;
        for (schema, name, owner) in [
            ("HumanResources", "Employee", "dbo"),
            ("HumanResources", "Department", "dbo"),
            ("Person", "Person", "dbo"),
            ("Person", "Address", "dbo"),
            ("Sales", "Customer", "dbo"),
            ("Sales", "SalesOrderHeader", "dbo"),
            ("Production", "Product", "dbo"),
            ("dbo", "AspNetUsers", "dbo"),
            ("dbo", "AuditLog", "dbo"),
        ] {
            let id = add(
                Some(tables),
                ObjectKind::Table,
                name,
                schema,
                owner,
                format!(
                    "Server[@Name='localhost']/Database[@Name='AdventureWorks']/Table[@Name='{name}' and @Schema='{schema}']"
                ),
            );
            if name == "Employee" && schema == "HumanResources" {
                employee_id = Some(id);
            }
        }
        if let Some(employee_id) = employee_id {
            let table_urn = "Server[@Name='localhost']/Database[@Name='AdventureWorks']/Table[@Name='Employee' and @Schema='HumanResources']";
            let cols = add(
                Some(employee_id),
                ObjectKind::Folder,
                "Columns",
                "",
                "",
                format!("{table_urn}/Columns"),
            );
            let idxs = add(
                Some(employee_id),
                ObjectKind::Folder,
                "Indexes",
                "",
                "",
                format!("{table_urn}/Indexes"),
            );
            add(
                Some(cols),
                ObjectKind::Column,
                "BusinessEntityID",
                "HumanResources",
                "",
                format!("{table_urn}/Column[@Name='BusinessEntityID']"),
            );
            add(
                Some(idxs),
                ObjectKind::Index,
                "PK_Employee",
                "HumanResources",
                "",
                format!("{table_urn}/Index[@Name='PK_Employee']"),
            );
        }

        let views = add(
            Some(db),
            ObjectKind::Folder,
            "Views",
            "",
            "",
            "Server[@Name='localhost']/Database[@Name='AdventureWorks']/View".into(),
        );
        for (schema, name) in [
            ("Sales", "vSalesPerson"),
            ("HumanResources", "vEmployee"),
            ("dbo", "vw_ActiveUsers"),
        ] {
            add(
                Some(views),
                ObjectKind::View,
                name,
                schema,
                "dbo",
                format!(
                    "Server[@Name='localhost']/Database[@Name='AdventureWorks']/View[@Name='{name}' and @Schema='{schema}']"
                ),
            );
        }

        let procs = add(
            Some(db),
            ObjectKind::Folder,
            "Stored Procedures",
            "",
            "",
            "Server[@Name='localhost']/Database[@Name='AdventureWorks']/StoredProcedure".into(),
        );
        for (schema, name) in [
            ("dbo", "uspGetEmployeeManagers"),
            ("dbo", "uspSearchProducts"),
            ("Sales", "uspUpdateOrder"),
        ] {
            add(
                Some(procs),
                ObjectKind::Procedure,
                name,
                schema,
                "dbo",
                format!(
                    "Server[@Name='localhost']/Database[@Name='AdventureWorks']/StoredProcedure[@Name='{name}' and @Schema='{schema}']"
                ),
            );
        }

        let synonyms = add(
            Some(db),
            ObjectKind::Folder,
            "Synonyms",
            "",
            "",
            "Server[@Name='localhost']/Database[@Name='AdventureWorks']/Synonym".into(),
        );
        add(
            Some(synonyms),
            ObjectKind::Synonym,
            "EmployeeAddress",
            "Person",
            "dbo",
            "Server[@Name='localhost']/Database[@Name='AdventureWorks']/Synonym[@Name='EmployeeAddress' and @Schema='Person']"
                .into(),
        );

        let triggers = add(
            Some(db),
            ObjectKind::Folder,
            "Triggers",
            "",
            "",
            "Server[@Name='localhost']/Database[@Name='AdventureWorks']/Trigger".into(),
        );
        add(
            Some(triggers),
            ObjectKind::Trigger,
            "dEmployee",
            "HumanResources",
            "dbo",
            "Server[@Name='localhost']/Database[@Name='AdventureWorks']/Trigger[@Name='dEmployee' and @Schema='HumanResources']"
                .into(),
        );

        if let Some(o) = objects.iter_mut().find(|o| Some(o.id) == employee_id) {
            o.create_date = "2009-01-14T10:02:11.230".into();
        }
        Self { objects }
    }

    pub fn by_id(&self, id: u64) -> Option<&CatalogObject> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn children(&self, parent: Option<u64>) -> Vec<&CatalogObject> {
        self.objects
            .iter()
            .filter(|o| o.parent_id == parent)
            .collect()
    }

    /// Walk `parent_id` until a [`ObjectKind::Database`] node; return its name.
    pub fn database_name_of(&self, id: u64) -> Option<&str> {
        let mut cur = self.by_id(id);
        while let Some(o) = cur {
            if o.kind == ObjectKind::Database {
                return Some(o.name.as_str());
            }
            cur = o.parent_id.and_then(|p| self.by_id(p));
        }
        None
    }

    pub fn database_count(&self) -> usize {
        self.objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Database)
            .count()
    }

    /// Non-context search: scan every leaf-like object (tables/views/procs/…)
    /// analogous to NonContextFilter* builders in ObjectExplorer.
    pub fn search(&self, conditions: &SearchConditions) -> Vec<&CatalogObject> {
        self.objects
            .iter()
            .filter(|o| {
                !matches!(
                    o.kind,
                    ObjectKind::Server | ObjectKind::Database | ObjectKind::Folder
                )
            })
            .filter(|o| o.matches(conditions))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{FilterItem, FilterOperator, SearchConditions};

    fn demo() -> Catalog {
        Catalog::demo()
    }

    #[test]
    fn object_kind_as_str_matches_smo_tokens() {
        assert_eq!(ObjectKind::Server.as_str(), "Server");
        assert_eq!(ObjectKind::Database.as_str(), "Database");
        assert_eq!(ObjectKind::Schema.as_str(), "Schema");
        assert_eq!(ObjectKind::Table.as_str(), "Table");
        assert_eq!(ObjectKind::View.as_str(), "View");
        assert_eq!(ObjectKind::Procedure.as_str(), "StoredProcedure");
        assert_eq!(ObjectKind::Function.as_str(), "UserDefinedFunction");
        assert_eq!(ObjectKind::Folder.as_str(), "Folder");
    }

    #[test]
    fn demo_has_server_db_folders_and_leaves() {
        let cat = demo();
        assert!(cat.objects.len() >= 20);
        assert!(cat.objects.iter().any(|o| o.kind == ObjectKind::Server));
        assert!(cat.objects.iter().any(|o| o.kind == ObjectKind::Database));
        assert!(cat.objects.iter().any(|o| o.kind == ObjectKind::Folder));
        assert!(cat.objects.iter().any(|o| o.kind == ObjectKind::Table));
        assert!(cat.objects.iter().any(|o| o.kind == ObjectKind::View));
        assert!(cat.objects.iter().any(|o| o.kind == ObjectKind::Procedure));
    }

    #[test]
    fn demo_urns_follow_smo_template() {
        let cat = demo();
        let server = cat
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Server)
            .unwrap();
        assert_eq!(server.urn, "Server[@Name='localhost']");
        let table = cat
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Table && o.name == "Employee")
            .unwrap();
        assert!(table.urn.contains("Database[@Name='AdventureWorks']"));
        assert!(table.urn.contains("@Name='Employee'"));
        assert!(table.urn.contains("@Schema='HumanResources'"));
    }

    #[test]
    fn by_id_and_children_tree_walk() {
        let cat = demo();
        let server = cat
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Server)
            .unwrap();
        assert_eq!(
            cat.by_id(server.id).map(|o| o.name.as_str()),
            Some("localhost")
        );
        assert!(cat.by_id(9_999_999).is_none());

        let roots = cat.children(None);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].kind, ObjectKind::Server);

        let dbs = cat.children(Some(server.id));
        assert_eq!(dbs.len(), 1);
        assert_eq!(dbs[0].name, "AdventureWorks");

        let folders = cat.children(Some(dbs[0].id));
        assert!(folders.iter().any(|o| o.name == "Tables"));
        assert!(folders.iter().any(|o| o.name == "Views"));
        assert!(folders.iter().any(|o| o.name == "Stored Procedures"));
    }

    #[test]
    fn search_skips_server_database_folder() {
        let cat = demo();
        let empty = SearchConditions::default();
        let hits = cat.search(&empty);
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|o| {
            !matches!(
                o.kind,
                ObjectKind::Server | ObjectKind::Database | ObjectKind::Folder
            )
        }));
    }

    #[test]
    fn search_by_name_contains() {
        let cat = demo();
        let sc = SearchConditions {
            items: vec![FilterItem::new("Name", FilterOperator::Contains, "user")],
        };
        let hits = cat.search(&sc);
        assert!(hits.iter().any(|o| o.name == "AspNetUsers"));
        assert!(hits.iter().any(|o| o.name == "vw_ActiveUsers"));
        assert!(!hits.iter().any(|o| o.name == "Product"));
    }

    #[test]
    fn search_by_schema_equal() {
        let cat = demo();
        let sc = SearchConditions {
            items: vec![FilterItem::new("Schema", FilterOperator::Equal, "Sales")],
        };
        let hits = cat.search(&sc);
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|o| o.schema.eq_ignore_ascii_case("Sales")));
        assert!(hits.iter().any(|o| o.name == "Customer"));
        assert!(hits.iter().any(|o| o.name == "uspUpdateOrder"));
    }

    #[test]
    fn search_and_across_name_and_schema() {
        let cat = demo();
        let sc = SearchConditions {
            items: vec![
                FilterItem::new("Name", FilterOperator::StartsWith, "usp"),
                FilterItem::new("Schema", FilterOperator::Equal, "dbo"),
            ],
        };
        let hits = cat.search(&sc);
        assert_eq!(hits.len(), 2);
        assert!(hits
            .iter()
            .all(|o| o.schema == "dbo" && o.name.starts_with("usp")));
    }

    #[test]
    fn search_type_filter_uses_smo_token() {
        let cat = demo();
        let sc = SearchConditions {
            items: vec![FilterItem::new(
                "Type",
                FilterOperator::Equal,
                "StoredProcedure",
            )],
        };
        let hits = cat.search(&sc);
        assert_eq!(hits.len(), 3);
        assert!(hits.iter().all(|o| o.kind == ObjectKind::Procedure));
    }

    #[test]
    fn search_type_synonym() {
        let cat = Catalog::demo();
        let sc = SearchConditions {
            items: vec![FilterItem::new("Type", FilterOperator::Equal, "Synonym")],
        };
        let hits = cat.search(&sc);
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|o| o.kind == ObjectKind::Synonym));
    }

    #[test]
    fn catalog_object_props_order() {
        let o = CatalogObject {
            id: 1,
            parent_id: None,
            kind: ObjectKind::Table,
            name: "T".into(),
            schema: "dbo".into(),
            owner: "sa".into(),
            urn: "x".into(),
            create_date: "2024-01-02T03:04:05.000".into(),
        };
        assert_eq!(
            o.props(),
            [
                ("Name", "T"),
                ("Schema", "dbo"),
                ("Owner", "sa"),
                ("Type", "Table"),
                ("Date", "2024-01-02T03:04:05.000"),
            ]
        );
    }

    #[test]
    fn demo_employee_has_date_and_filter_matches() {
        let cat = Catalog::demo();
        let sc = SearchConditions {
            items: vec![FilterItem::new("Date", FilterOperator::StartsWith, "2009")],
        };
        let hits = cat.search(&sc);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Employee");
    }

    #[test]
    fn empty_date_never_matches_date_filters() {
        let cat = Catalog::demo();
        for op in FilterOperator::ALL {
            let mut item = FilterItem::new("Date", op, "2020");
            item.value2 = "2030".into();
            let sc = SearchConditions { items: vec![item] };
            let hits = cat.search(&sc);
            assert!(
                hits.iter().all(|o| !o.create_date.is_empty()),
                "{op}: matched an object with empty date"
            );
        }
        let sc = SearchConditions {
            items: vec![FilterItem::new("Date", FilterOperator::LessThan, "2020")],
        };
        assert_eq!(cat.search(&sc).len(), 1); // only Employee (2009)
    }

    #[test]
    fn multi_db_catalog_structure() {
        let mk = |id, parent_id, kind, name: &str| CatalogObject {
            id,
            parent_id,
            kind,
            name: name.into(),
            schema: String::new(),
            owner: String::new(),
            urn: String::new(),
            create_date: String::new(),
        };
        let cat = Catalog {
            objects: vec![
                mk(1, None, ObjectKind::Server, "srv"),
                mk(2, Some(1), ObjectKind::Database, "a"),
                mk(3, Some(1), ObjectKind::Database, "b"),
                mk(4, Some(2), ObjectKind::Folder, "Tables"),
            ],
        };
        let dbs = cat.children(Some(1));
        assert_eq!(dbs.len(), 2);
        // Unloaded database "b" has no children; "a" does.
        assert!(cat.children(Some(3)).is_empty());
        assert_eq!(cat.children(Some(2)).len(), 1);
    }

    #[test]
    fn object_kind_new_variants_as_str() {
        assert_eq!(ObjectKind::Column.as_str(), "Column");
        assert_eq!(ObjectKind::Index.as_str(), "Index");
        assert_eq!(ObjectKind::Synonym.as_str(), "Synonym");
        assert_eq!(ObjectKind::Trigger.as_str(), "Trigger");
    }

    #[test]
    fn search_includes_synonym_and_trigger() {
        let cat = Catalog::demo();
        let sc = SearchConditions::default();
        let hits = cat.search(&sc);
        assert!(hits.iter().any(|o| o.kind == ObjectKind::Synonym));
        assert!(hits.iter().any(|o| o.kind == ObjectKind::Trigger));
    }

    #[test]
    fn demo_table_has_column_and_index_children() {
        let cat = Catalog::demo();
        let table = cat
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Table && o.name == "Employee")
            .expect("Employee");
        let folders: Vec<_> = cat.children(Some(table.id));
        assert!(folders.iter().any(|o| o.name == "Columns"));
        assert!(folders.iter().any(|o| o.name == "Indexes"));
        let cols_folder = folders.iter().find(|o| o.name == "Columns").unwrap();
        let idx_folder = folders.iter().find(|o| o.name == "Indexes").unwrap();
        assert!(cat
            .children(Some(cols_folder.id))
            .iter()
            .any(|o| o.kind == ObjectKind::Column));
        assert!(cat
            .children(Some(idx_folder.id))
            .iter()
            .any(|o| o.kind == ObjectKind::Index));
    }

    #[test]
    fn database_name_of_walks_to_database() {
        let cat = Catalog::demo();
        let table = cat
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Table && o.name == "Employee")
            .unwrap();
        assert_eq!(cat.database_name_of(table.id), Some("AdventureWorks"));
        let server = cat
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Server)
            .unwrap();
        assert_eq!(cat.database_name_of(server.id), None);
    }

    #[test]
    fn serde_deserializes_without_create_date() {
        let json = r#"{"id":1,"parent_id":null,"kind":"Table","name":"t","schema":"dbo","owner":"dbo","urn":"u"}"#;
        let o: CatalogObject = serde_json::from_str(json).unwrap();
        assert!(o.create_date.is_empty());
    }

    #[test]
    fn serde_roundtrip_catalog_object() {
        let o = CatalogObject {
            id: 42,
            parent_id: Some(1),
            kind: ObjectKind::View,
            name: "v1".into(),
            schema: "dbo".into(),
            owner: "dbo".into(),
            urn: "Server[@Name='x']".into(),
            create_date: String::new(),
        };
        let json = serde_json::to_string(&o).unwrap();
        let back: CatalogObject = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, 42);
        assert_eq!(back.kind, ObjectKind::View);
        assert_eq!(back.urn, o.urn);
    }
}
