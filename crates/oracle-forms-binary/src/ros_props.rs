//! Static ROS.60050 property-ID dictionary mapping binary property IDs to
//! Forms2XML attribute names and value decoders.
//!
//! Purpose: Static ROS.60050 property-ID dictionary and enum mappings.
//! Key Input Dependencies: None (std/crate-internal only)
//! Index: RosPropKind, lookup_ros_prop, enum_str

/// Value encoding of a serialized `ROS.60050` property entry `(pid: u16, pval: u32)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosPropKind {
    /// Virtual-stream pointer to a NUL-terminated string.
    Str,
    /// Boolean encoded as `0` / non-zero `u32`.
    Bool,
    /// Unsigned integer (`u32`).
    UInt,
    /// Signed 32-bit integer (`pval as i32`).
    SignedInt,
    /// Enumerated integer mapped to Forms2XML attribute string.
    Enum(&'static [(u32, &'static str)]),
    /// Internal pointer / container-list / string-table ID (not an XML attribute).
    SkipInternal,
}

pub const REAL_UNIT_ENUM: &[(u32, &str)] = &[
    (0, "Centimeter"),
    (1, "Inch"),
    (2, "Pixel"),
    (3, "Point"),
    (7, "Point"),
];

pub const COORD_SYSTEM_ENUM: &[(u32, &str)] = &[
    (0, "Character"),
    (1, "Real"),
];

pub const ITEM_TYPE_ENUM: &[(u32, &str)] = &[
    (0, "Chart Item"),
    (1, "Check Box"),
    (2, "Display Item"),
    (3, "Hierarchical Tree"),
    (4, "Image"),
    (5, "List Item"),
    (6, "Push Button"),
    (7, "Radio Group"),
    (8, "Text Item"),
    (9, "User Area"),
    (10, "Bean Area"),
];

pub const DATA_TYPE_ENUM: &[(u32, &str)] = &[
    (0, "Char"),
    (1, "Number"),
    (2, "Date"),
    (3, "Alpha"),
    (4, "Integer"),
    (5, "Datetime"),
    (6, "Long"),
    (7, "Rnumber"),
    (8, "Jdate"),
    (9, "Edate"),
    (10, "Time"),
    (11, "Rinteger"),
    (12, "Money"),
    (13, "Rmoney"),
    (14, "Object Ref"),
];

pub const CANVAS_TYPE_ENUM: &[(u32, &str)] = &[
    (0, "Content"),
    (1, "Stacked"),
    (2, "Vertical Toolbar"),
    (3, "Horizontal Toolbar"),
    (4, "Tab"),
];

pub const GRAPHIC_TYPE_ENUM: &[(u32, &str)] = &[
    (0, "Arc"),
    (1, "Image"),
    (2, "Line"),
    (3, "Polygon"),
    (4, "Rectangle"),
    (5, "Rounded Rectangle"),
    (6, "Text"),
    (7, "Group"),
    (8, "Frame"),
];

pub const ALERT_STYLE_ENUM: &[(u32, &str)] = &[
    (0, "Stop"),
    (1, "Caution"),
    (2, "Note"),
];

pub const DEFAULT_BUTTON_ENUM: &[(u32, &str)] = &[
    (0, "Button 1"),
    (1, "Button 2"),
    (2, "Button 3"),
];

pub const NAV_STYLE_ENUM: &[(u32, &str)] = &[
    (0, "Same Record"),
    (1, "Change Record"),
    (2, "Change Data Block"),
];

pub const RECORD_ORIENTATION_ENUM: &[(u32, &str)] = &[
    (0, "Vertical"),
    (1, "Horizontal"),
];

pub const DATA_SOURCE_TYPE_ENUM: &[(u32, &str)] = &[
    (0, "None"),
    (1, "Table"),
    (2, "Procedure"),
    (3, "Transactional Triggers"),
    (4, "FROM clause query"),
];

pub const DML_DATA_TYPE_ENUM: &[(u32, &str)] = &[
    (0, "None"),
    (1, "Table"),
    (2, "Procedure"),
    (3, "Transactional Triggers"),
];

pub const JUSTIFICATION_ENUM: &[(u32, &str)] = &[
    (0, "Left"),
    (1, "Right"),
    (2, "Center"),
    (3, "Start"),
    (4, "End"),
];

pub const BEVEL_ENUM: &[(u32, &str)] = &[
    (0, "Raised"),
    (1, "Lowered"),
    (2, "None"),
    (3, "Inset"),
    (4, "Outset"),
    (5, "Plain"),
];

pub const COMPRESSION_ENUM: &[(u32, &str)] = &[
    (0, "None"),
    (1, "Minimum"),
    (2, "Low"),
    (3, "Medium"),
    (4, "High"),
    (5, "Maximum"),
];

pub const FONT_WEIGHT_ENUM: &[(u32, &str)] = &[
    (0, "Ultralight"),
    (1, "Extralight"),
    (2, "Light"),
    (3, "Demilight"),
    (4, "Medium"),
    (5, "Demibold"),
    (6, "Bold"),
    (7, "Extrabold"),
    (8, "Ultrabold"),
];

pub const FONT_STYLE_ENUM: &[(u32, &str)] = &[
    (0, "Plain"),
    (1, "Italic"),
    (2, "Oblique"),
    (3, "Underline"),
    (4, "Outline"),
    (5, "Shadow"),
    (6, "Inverted"),
    (7, "Overstrike"),
    (8, "Blink"),
];

pub const FONT_SPACING_ENUM: &[(u32, &str)] = &[
    (0, "Ultrawide"),
    (1, "Extrawide"),
    (2, "Wide"),
    (3, "Normal"),
    (4, "Narrow"),
    (5, "Extranarrow"),
    (6, "Ultranarrow"),
    (7, "Ultradense"),
    (8, "Dense"),
];

pub const EXEC_HIERARCHY_ENUM: &[(u32, &str)] = &[
    (0, "Override"),
    (1, "Before"),
    (2, "After"),
];

pub const COMMAND_TYPE_ENUM: &[(u32, &str)] = &[
    (0, "Null"),
    (1, "Menu"),
    (2, "PL/SQL"),
    (3, "Plus"),
    (4, "Form"),
    (5, "Macro"),
];

pub const MENU_ITEM_TYPE_ENUM: &[(u32, &str)] = &[
    (0, "Plain"),
    (1, "Check"),
    (2, "Radio"),
    (3, "Separator"),
    (4, "Magic"),
];

/// Looks up the Forms2XML attribute name and decoding kind for a `(obj_type, pid)` pair.
pub fn lookup_ros_prop(obj_type: u16, pid: u16) -> Option<(&'static str, RosPropKind)> {
    // Object-type-specific overrides where PIDs are overloaded across ROS types.
    match (obj_type, pid) {
        (0x03, 0x000a) => return Some(("AlertMessage", RosPropKind::Str)),
        (0x03, 0x000b) => return Some(("AlertStyle", RosPropKind::Enum(ALERT_STYLE_ENUM))),
        (0x0b, 0x002d) => return Some(("CanvasType", RosPropKind::Enum(CANVAS_TYPE_ENUM))),
        (0x0b, 0x0164) | (0x0b, 0x016f) => return Some(("WindowName", RosPropKind::Str)),
        (0x16, 0x003b) => return Some(("ConsoleWindow", RosPropKind::Str)),
        (0x16, 0x005d) => return Some(("FirstNavigationBlockName", RosPropKind::Str)),
        (0x16, 0x02e0) => return Some(("RuntimeComp", RosPropKind::Str)),
        (0x1e, 0x00f5) => return Some(("PrimaryKey", RosPropKind::Bool)),
        (0x2a, 0x0036) => return Some(("CommandType", RosPropKind::Enum(COMMAND_TYPE_ENUM))),
        (0x2a, 0x00c1) => return Some(("MenuItemType", RosPropKind::Enum(MENU_ITEM_TYPE_ENUM))),
        (0x3a, 0x01d3) => return Some(("", RosPropKind::SkipInternal)), // ProgramUnitType bitmask
        _ => {}
    }

    // Container list pointers, string-table internals, and PL/SQL blob IDs.
    if matches!(
        pid,
        0x0004 | 0x000d | 0x001b | 0x001e | 0x001f | 0x0020 | 0x0051 | 0x0071 | 0x0089 | 0x009f
            | 0x00a4 | 0x00b7 | 0x00b9 | 0x00f7 | 0x00f9 | 0x0106 | 0x0110 | 0x0118 | 0x013a
            | 0x0163 | 0x016b | 0x0199 | 0x01cd | 0x01cf | 0x01d0 | 0x01d1 | 0x01e6 | 0x01e7
            | 0x01f1 | 0x01f4 | 0x023f | 0x02e2..=0x02eb | 0x03a3..=0x03a6
    ) {
        return Some(("", RosPropKind::SkipInternal));
    }

    Some(match pid {
        0x0002 => ("AccessKey", RosPropKind::Str),
        0x0005 => ("Justification", RosPropKind::Enum(JUSTIFICATION_ENUM)),
        0x0006 => ("Button1Label", RosPropKind::Str),
        0x0007 => ("Button2Label", RosPropKind::Str),
        0x0008 => ("Button3Label", RosPropKind::Str),
        0x0009 => ("DefaultAlertButton", RosPropKind::Enum(DEFAULT_BUTTON_ENUM)),
        0x0011 => ("AutoHint", RosPropKind::Bool),
        0x0013 => ("BackColor", RosPropKind::Str),
        0x0014 => ("DatabaseItem", RosPropKind::Bool),
        0x0017 => ("Bevel", RosPropKind::Enum(BEVEL_ENUM)),
        0x0021 => ("CanvasName", RosPropKind::Str),
        0x002a => ("CheckedValue", RosPropKind::Str),
        0x0041 => ("DeleteAllowed", RosPropKind::Bool),
        0x004a => ("Visible", RosPropKind::Bool),
        0x004e => ("ViewportXPosition", RosPropKind::SignedInt),
        0x004f => ("ViewportYPosition", RosPropKind::SignedInt),
        0x0056 => ("Enabled", RosPropKind::Bool),
        0x0061 => ("MenuModule", RosPropKind::Str),
        0x0067 => ("FontName", RosPropKind::Str),
        0x0068 => ("FontSize", RosPropKind::UInt),
        0x0069 => ("FontStyle", RosPropKind::Enum(FONT_STYLE_ENUM)),
        0x006a => ("FontWeight", RosPropKind::Enum(FONT_WEIGHT_ENUM)),
        0x006b => ("FontSpacing", RosPropKind::Enum(FONT_SPACING_ENUM)),
        0x006c => ("ForegroundColor", RosPropKind::Str),
        0x006d => ("FrameTitle", RosPropKind::Str),
        0x0079 => ("Height", RosPropKind::UInt),
        0x007b => ("Hint", RosPropKind::Str),
        0x007d => ("ShowHorizontalScrollbar", RosPropKind::Bool),
        0x007e => ("HorizontalToolbarCanvasName", RosPropKind::Str),
        0x0086 => ("InsertAllowed", RosPropKind::Bool),
        0x008b => ("ItemType", RosPropKind::Enum(ITEM_TYPE_ENUM)),
        0x008c => ("ItemsDisplay", RosPropKind::UInt),
        0x008d => ("DataType", RosPropKind::Enum(DATA_TYPE_ENUM)),
        0x0093 => ("GraphicsType", RosPropKind::Enum(GRAPHIC_TYPE_ENUM)),
        0x0096 => ("KeyboardNavigable", RosPropKind::Bool),
        0x0097 => ("Label", RosPropKind::Str),
        0x009c => ("LibraryLocation", RosPropKind::Str),
        0x00a5 => ("LovName", RosPropKind::Str),
        0x00af => ("MaximumLength", RosPropKind::UInt),
        0x00bc => ("CompressionQuality", RosPropKind::Enum(COMPRESSION_ENUM)),
        0x00c2 => ("Modal", RosPropKind::Bool),
        0x00c3 => ("MouseNavigate", RosPropKind::Bool),
        0x00d3 => ("Name", RosPropKind::Str),
        0x00d6 => ("NavigationStyle", RosPropKind::Enum(NAV_STYLE_ENUM)),
        0x00dd => ("NextNavigationBlockName", RosPropKind::Str),
        0x00f1 => ("OrderByClause", RosPropKind::Str),
        0x00f4 => ("PreviousNavigationBlockName", RosPropKind::Str),
        0x0101 => ("QueryAllowed", RosPropKind::Bool),
        0x0107 => ("RaiseOnEnter", RosPropKind::Bool),
        0x010c => ("RecordsDisplayCount", RosPropKind::UInt),
        0x010e => ("DistanceBetweenRecords", RosPropKind::UInt),
        0x0112 => ("RecordOrientation", RosPropKind::Enum(RECORD_ORIENTATION_ENUM)),
        0x011b => ("Required", RosPropKind::Bool),
        0x0122 => ("ScrollbarCanvasName", RosPropKind::Str),
        0x0125 => ("ScrollbarWidth", RosPropKind::UInt),
        0x0128 => ("ShowScrollbar", RosPropKind::Bool),
        0x0133 => ("Title", RosPropKind::Str),
        0x013b => ("ExecutionHierarchy", RosPropKind::Enum(EXEC_HIERARCHY_ENUM)),
        0x0141 => ("UncheckedValue", RosPropKind::Str),
        0x0142 => ("UpdateAllowed", RosPropKind::Bool),
        0x0147 => ("Text", RosPropKind::Str),
        0x0148 => ("VisualAttributeName", RosPropKind::Str),
        0x0158 => ("ColumnName", RosPropKind::Str),
        0x015b => ("ShowVerticalScrollbar", RosPropKind::Bool),
        0x015e => ("ViewportHeight", RosPropKind::UInt),
        0x015f => ("ViewportWidth", RosPropKind::UInt),
        0x0160 => ("ViewportXPositionOnCanvas", RosPropKind::SignedInt),
        0x0161 => ("ViewportYPositionOnCanvas", RosPropKind::SignedInt),
        0x0167 => ("WhereClause", RosPropKind::Str),
        0x0169 => ("Width", RosPropKind::UInt),
        0x0172 => ("PrimaryCanvas", RosPropKind::Str),
        0x0174 => ("XPosition", RosPropKind::SignedInt),
        0x0175 => ("YPosition", RosPropKind::SignedInt),
        0x018a => ("Prompt", RosPropKind::Str),
        0x01f2 => ("TabPageName", RosPropKind::Str),
        0x0208 => ("QueryDataSourceType", RosPropKind::Enum(DATA_SOURCE_TYPE_ENUM)),
        0x020a => ("QueryDataSourceName", RosPropKind::Str),
        0x0210 => ("DatabaseBlock", RosPropKind::Bool),
        0x0216 => ("DMLDataType", RosPropKind::Enum(DML_DATA_TYPE_ENUM)),
        0x0217 => ("DMLDataTargetName", RosPropKind::Str),
        0x02fa => ("ScrollbarLength", RosPropKind::UInt),
        _ => return None,
    })
}

pub fn enum_str(table: &[(u32, &'static str)], val: u32) -> Option<&'static str> {
    table.iter().find(|&&(k, _)| k == val).map(|&(_, s)| s)
}
