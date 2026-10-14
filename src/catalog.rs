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
}

impl CatalogObject {
    pub fn props(&self) -> [(&str, &str); 4] {
        [
            ("Name", self.name.as_str()),
            ("Schema", self.schema.as_str()),
            ("Owner", self.owner.as_str()),
            ("Type", self.kind.as_str()),
        ]
    }

    pub fn matches(&self, conditions: &SearchConditions) -> bool {
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
            format!("Server[@Name='localhost']/Database[@Name='AdventureWorks']/Table"),
        );
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
            add(
                Some(tables),
                ObjectKind::Table,
                name,
                schema,
                owner,
                format!(
                    "Server[@Name='localhost']/Database[@Name='AdventureWorks']/Table[@Name='{name}' and @Schema='{schema}']"
                ),
            );
        }

        let views = add(
            Some(db),
            ObjectKind::Folder,
            "Views",
            "",
            "",
            format!("Server[@Name='localhost']/Database[@Name='AdventureWorks']/View"),
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
            format!("Server[@Name='localhost']/Database[@Name='AdventureWorks']/StoredProcedure"),
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

    /// Non-context search: scan every leaf-like object (tables/views/procs/…)
    /// analogous to NonContextFilter* builders in ObjectExplorer.
    pub fn search(&self, conditions: &SearchConditions) -> Vec<&CatalogObject> {
        self.objects
            .iter()
            .filter(|o| {
                !matches!(o.kind, ObjectKind::Server | ObjectKind::Database | ObjectKind::Folder)
            })
            .filter(|o| o.matches(conditions))
            .collect()
    }
}
