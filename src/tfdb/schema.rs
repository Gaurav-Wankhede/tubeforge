//! Table/column schema model for the TubeForge DB engine.
//!
//! A `Schema` is a named set of typed columns. Each table has one primary key
//! column (TEXT) plus any number of typed value columns. This is the whole
//! "schema" — no SQL DDL, no ALTER beyond additive column sets.

use serde::{Deserialize, Serialize};

/// The type of a value column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColType {
    Text,
    Int,
    Float,
    Bool,
    /// Raw bytes (embeddings, thumbnails metadata).
    Blob,
    /// A JSON-encoded nested value (arrays/objects) stored as text.
    Json,
}

/// One column definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Col {
    pub name: String,
    pub ty: ColType,
    /// Whether values in this column must be unique across rows.
    pub unique: bool,
}

/// The column set for one table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub pk: String,
    pub cols: Vec<Col>,
}

impl TableSchema {
    pub fn new(name: impl Into<String>, pk: impl Into<String>) -> Self {
        TableSchema {
            name: name.into(),
            pk: pk.into(),
            cols: Vec::new(),
        }
    }

    pub fn text(mut self, name: impl Into<String>) -> Self {
        self.cols.push(Col {
            name: name.into(),
            ty: ColType::Text,
            unique: false,
        });
        self
    }

    pub fn int(mut self, name: impl Into<String>) -> Self {
        self.cols.push(Col {
            name: name.into(),
            ty: ColType::Int,
            unique: false,
        });
        self
    }

    pub fn float(mut self, name: impl Into<String>) -> Self {
        self.cols.push(Col {
            name: name.into(),
            ty: ColType::Float,
            unique: false,
        });
        self
    }

    pub fn boolean(mut self, name: impl Into<String>) -> Self {
        self.cols.push(Col {
            name: name.into(),
            ty: ColType::Bool,
            unique: false,
        });
        self
    }

    pub fn blob(mut self, name: impl Into<String>) -> Self {
        self.cols.push(Col {
            name: name.into(),
            ty: ColType::Blob,
            unique: false,
        });
        self
    }

    pub fn json(mut self, name: impl Into<String>) -> Self {
        self.cols.push(Col {
            name: name.into(),
            ty: ColType::Json,
            unique: false,
        });
        self
    }

    /// Mark the column `name` as unique across rows.
    pub fn unique(mut self, name: &str) -> Self {
        if let Some(c) = self.cols.iter_mut().find(|c| c.name == name) {
            c.unique = true;
        }
        self
    }

    pub fn col_type(&self, name: &str) -> Option<ColType> {
        self.cols.iter().find(|c| c.name == name).map(|c| c.ty)
    }

    pub fn has(&self, name: &str) -> bool {
        self.cols.iter().any(|c| c.name == name) || self.pk == name
    }
}
