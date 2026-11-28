//! Session-scoped runtime objects created by built-ins: parameter lists, record groups and
//! timers, plus the session clock they share.
//!
//! Ids follow the rest of the runtime: an object id is the object's name (an unnamed
//! parameter list gets a generated name), and `NULL` stands for "not found" so
//! `ID_NULL(FIND_x(...))` behaves like Forms.
//!
//! Purpose: Module implementation (objects.rs).
//! Key Input Dependencies: oracle_forms_core, std
//! Index: ParamKind, ParamEntry, ParameterList, GroupColumnType, GroupColumn, RecordGroup, Timer, RuntimeObjects, from_module, now_ms, advance_clock, next_anonymous_name

use crate::value::OracleValue;
use oracle_forms_core::model::{ModuleIndex, RecordGroupInfo};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Text,
    Data,
}

impl ParamKind {
    /// The Forms constant name (`TEXT_PARAMETER` / `DATA_PARAMETER`).
    pub fn constant(self) -> &'static str {
        match self {
            ParamKind::Text => "TEXT_PARAMETER",
            ParamKind::Data => "DATA_PARAMETER",
        }
    }

    pub fn from_constant(text: &str) -> Option<Self> {
        match text.trim().to_ascii_uppercase().as_str() {
            "TEXT_PARAMETER" => Some(ParamKind::Text),
            "DATA_PARAMETER" => Some(ParamKind::Data),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamEntry {
    pub key: String,
    pub kind: ParamKind,
    /// The text value, or (for `DATA_PARAMETER`) the record group name.
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParameterList {
    pub name: String,
    pub entries: Vec<ParamEntry>,
}

impl ParameterList {
    pub fn position(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.key.eq_ignore_ascii_case(key))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupColumnType {
    Char,
    Number,
    Date,
    Long,
}

impl GroupColumnType {
    pub fn from_constant(text: &str) -> Option<Self> {
        match text.trim().to_ascii_uppercase().as_str() {
            "CHAR_COLUMN" => Some(Self::Char),
            "NUMBER_COLUMN" => Some(Self::Number),
            "DATE_COLUMN" => Some(Self::Date),
            "LONG_COLUMN" => Some(Self::Long),
            _ => None,
        }
    }

    /// Maps a Forms Builder `ColumnDataType` attribute.
    pub fn from_module_type(text: Option<&str>) -> Self {
        match text.map(|t| t.trim().to_ascii_lowercase()).as_deref() {
            Some("number") => Self::Number,
            Some("date") => Self::Date,
            Some("long") => Self::Long,
            _ => Self::Char,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroupColumn {
    pub name: String,
    pub data_type: GroupColumnType,
}

#[derive(Debug, Clone, Default)]
pub struct RecordGroup {
    pub name: String,
    pub columns: Vec<GroupColumn>,
    /// One `Vec` per row, one value per column.
    pub rows: Vec<Vec<OracleValue>>,
    /// Default query (design-time or `CREATE_GROUP_FROM_QUERY`).
    pub query: Option<String>,
    /// True once rows were loaded (by a query or static data); static design-time groups
    /// start populated.
    pub populated: bool,
    /// Created by `CREATE_GROUP*` (only these can be deleted, as in Forms).
    pub runtime_created: bool,
}

impl RecordGroup {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Builds the runtime group from a design-time definition (static rows are loaded).
    pub fn from_definition(def: &RecordGroupInfo) -> Self {
        let columns: Vec<GroupColumn> = def
            .columns
            .iter()
            .map(|c| GroupColumn {
                name: c.clone(),
                data_type: GroupColumnType::from_module_type(
                    def.column_specs.iter().find(|s| s.name.eq_ignore_ascii_case(c)).and_then(|s| s.data_type.as_deref()),
                ),
            })
            .collect();
        let row_count = def.column_specs.iter().map(|s| s.rows.len()).max().unwrap_or(0);
        let is_static = def.query_text.is_none() && row_count > 0;
        let mut rows = Vec::new();
        if is_static {
            for r in 0..row_count {
                rows.push(
                    columns
                        .iter()
                        .map(|c| {
                            let raw = def
                                .column_specs
                                .iter()
                                .find(|s| s.name.eq_ignore_ascii_case(&c.name))
                                .and_then(|s| s.rows.get(r));
                            match raw {
                                None => OracleValue::Null,
                                Some(v) => match c.data_type {
                                    GroupColumnType::Number => OracleValue::number_from_str(v),
                                    _ => OracleValue::varchar2(v.clone()),
                                },
                            }
                        })
                        .collect(),
                );
            }
        }
        RecordGroup { name: def.name.clone(), columns, rows, query: def.query_text.clone(), populated: is_static, runtime_created: false }
    }
}

/// A timer created with `CREATE_TIMER`.
#[derive(Debug, Clone)]
pub struct Timer {
    pub name: String,
    pub interval_ms: i64,
    pub repeat: bool,
    /// Session-clock time at which the timer next expires; `None` once a non-repeating
    /// timer has fired (it stays defined, as in Forms, until deleted or reset by SET_TIMER).
    pub due_at_ms: Option<i64>,
}

/// All session-scoped objects plus the session clock.
#[derive(Debug)]
pub struct RuntimeObjects {
    pub parameter_lists: HashMap<String, ParameterList>,
    pub record_groups: HashMap<String, RecordGroup>,
    pub timers: HashMap<String, Timer>,
    /// Name of the timer whose `WHEN-TIMER-EXPIRED` is running (or ran last).
    pub expired_timer: Option<String>,
    /// OUT values produced by the built-in that just ran, assigned by the interpreter.
    pub pending_out: Vec<OracleValue>,
    /// `SET_LOV_PROPERTY` overrides: LOV name -> property -> value.
    pub lov_overrides: HashMap<String, HashMap<String, OracleValue>>,
    started: Instant,
    skew_ms: i64,
    anonymous: u64,
}

impl Default for RuntimeObjects {
    fn default() -> Self {
        Self {
            parameter_lists: HashMap::new(),
            record_groups: HashMap::new(),
            timers: HashMap::new(),
            expired_timer: None,
            pending_out: Vec::new(),
            lov_overrides: HashMap::new(),
            started: Instant::now(),
            skew_ms: 0,
            anonymous: 0,
        }
    }
}

impl RuntimeObjects {
    /// Creates the design-time record groups of `module` (the `DEFAULT` parameter list
    /// does not exist until a form creates it).
    pub fn from_module(module: &ModuleIndex) -> Self {
        let mut o = Self::default();
        for rg in &module.record_groups {
            o.record_groups.insert(rg.name.to_ascii_uppercase(), RecordGroup::from_definition(rg));
        }
        o
    }

    /// Milliseconds on the session clock (real time since creation plus any test skew).
    pub fn now_ms(&self) -> i64 {
        self.started.elapsed().as_millis() as i64 + self.skew_ms
    }

    /// Moves the session clock forward (used by tests and by hosts that replay time).
    pub fn advance_clock(&mut self, ms: i64) {
        self.skew_ms += ms;
    }

    /// A generated name for an unnamed object (`CREATE_PARAMETER_LIST(NULL)`).
    pub fn next_anonymous_name(&mut self, prefix: &str) -> String {
        self.anonymous += 1;
        format!("{}${}", prefix, self.anonymous)
    }
}
