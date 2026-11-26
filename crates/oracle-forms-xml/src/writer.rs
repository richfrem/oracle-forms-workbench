//! Purpose: Serializer for Forms2XML or ROS.60050 binary output.
//! Key Input Dependencies: oracle_forms_core
//! Index: FormsXmlWriter, write_string, write_to_file, write_form_module, write_block, write_item, write_canvas, write_menu_module, write_object_library, append_extra_attrs, write_raw_xml_node, is_valid_xml_char, escape_attr, normalize_module_version, append_extra_attrs_skips_case_insensitive_duplicates, escape_attr_strips_illegal_xml_control_chars, normalize_module_version_requires_numeric_xsd_long

use oracle_forms_core::model::*;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::Path;

pub struct FormsXmlWriter;

impl FormsXmlWriter {
    /// Serializes a ModuleIndex back into Forms2XML XML string.
    pub fn write_string(module: &ModuleIndex) -> String {
        let mut out = String::with_capacity(32 * 1024);
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

        let version = normalize_module_version(module.forms_version.as_deref());

        let _ = writeln!(
            out,
            "<Module version=\"{}\" xmlns=\"http://xmlns.oracle.com/Forms\">",
            escape_attr(version)
        );

        match module.key.module_type {
            ModuleType::Form => Self::write_form_module(&mut out, module),
            ModuleType::Menu => Self::write_menu_module(&mut out, module),
            ModuleType::ObjectLibrary => Self::write_object_library(&mut out, module),
            _ => Self::write_form_module(&mut out, module),
        }

        out.push_str("</Module>\n");
        out
    }

    /// Serializes a ModuleIndex and saves it to a file.
    pub fn write_to_file(module: &ModuleIndex, path: &Path) -> std::io::Result<()> {
        let content = Self::write_string(module);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)
    }

    fn write_form_module(out: &mut String, module: &ModuleIndex) {
        let form_name = &module.key.name;
        let mut attrs = vec![format!("Name=\"{}\"", escape_attr(form_name))];

        if let Some(title) = &module.title {
            attrs.push(format!("Title=\"{}\"", escape_attr(title)));
        }
        if let Some(cw) = &module.console_window {
            attrs.push(format!("ConsoleWindow=\"{}\"", escape_attr(cw)));
        }
        if let Some(menu) = &module.menu_module {
            attrs.push(format!("MenuModule=\"{}\"", escape_attr(menu)));
        }
        if let Some(fnb) = &module.first_navigation_block_name {
            attrs.push(format!("FirstNavigationBlockName=\"{}\"", escape_attr(fnb)));
        }
        if let Some(rc) = &module.runtime_comp {
            attrs.push(format!("RuntimeComp=\"{}\"", escape_attr(rc)));
        }
        append_extra_attrs(&mut attrs, &module.extra_attrs);

        let _ = writeln!(out, "  <FormModule {}>", attrs.join(" "));

        // Coordinate Info
        let coord = &module.coordinate;
        if coord.present || coord.coordinate_system.is_some() {
            let mut coord_attrs = Vec::new();
            if let Some(cs) = &coord.coordinate_system {
                coord_attrs.push(format!("CoordinateSystem=\"{}\"", escape_attr(cs)));
            }
            if let Some(ru) = &coord.real_unit {
                coord_attrs.push(format!("RealUnit=\"{}\"", escape_attr(ru)));
            }
            coord_attrs.push(format!("CharacterCellWidth=\"{}\"", coord.character_cell_width));
            coord_attrs.push(format!("CharacterCellHeight=\"{}\"", coord.character_cell_height));
            coord_attrs.push(format!("DefaultFontScaling=\"{}\"", coord.default_font_scaling));
            let _ = writeln!(out, "    <Coordinate {}/>", coord_attrs.join(" "));
        }

        // Alerts
        for alert in &module.alerts {
            let mut a_attrs = vec![format!("Name=\"{}\"", escape_attr(&alert.name))];
            if let Some(msg) = &alert.message {
                a_attrs.push(format!("AlertMessage=\"{}\"", escape_attr(msg)));
            }
            let _ = writeln!(out, "    <Alert {}/>", a_attrs.join(" "));
        }

        // Attached Libraries
        for lib in &module.attached_libraries {
            let mut l_attrs = vec![format!("Name=\"{}\"", escape_attr(&lib.name))];
            if let Some(loc) = &lib.library_location {
                l_attrs.push(format!("LibraryLocation=\"{}\"", escape_attr(loc)));
            }
            let _ = writeln!(out, "    <AttachedLibrary {}/>", l_attrs.join(" "));
        }

        // Blocks
        for block in &module.blocks {
            Self::write_block(out, block, module);
        }

        // Canvases
        for canvas in &module.canvases {
            Self::write_canvas(out, canvas, module);
        }

        // Windows
        for window in &module.windows {
            let mut w_attrs = vec![format!("Name=\"{}\"", escape_attr(&window.name))];
            if let Some(title) = &window.title {
                w_attrs.push(format!("Title=\"{}\"", escape_attr(title)));
            }
            if let Some(modal) = window.modal {
                w_attrs.push(format!("Modal=\"{}\"", modal));
            }
            if let Some(w) = window.width {
                w_attrs.push(format!("Width=\"{}\"", w));
            }
            if let Some(h) = window.height {
                w_attrs.push(format!("Height=\"{}\"", h));
            }
            if let Some(tc) = &window.horizontal_toolbar_canvas_name {
                w_attrs.push(format!("HorizontalToolbarCanvasName=\"{}\"", escape_attr(tc)));
            }
            if let Some(vc) = &window.vertical_toolbar_canvas_name {
                w_attrs.push(format!("VerticalToolbarCanvasName=\"{}\"", escape_attr(vc)));
            }
            let _ = writeln!(out, "    <Window {}/>", w_attrs.join(" "));
        }

        // LOVs & RecordGroups
        for rg in &module.record_groups {
            let mut rg_attrs = vec![format!("Name=\"{}\"", escape_attr(&rg.name))];
            if let Some(q) = &rg.query_text {
                rg_attrs.push(format!("RecordGroupType=\"Query\" RecordGroupQuery=\"{}\"", escape_attr(q)));
            }
            let _ = writeln!(out, "    <RecordGroup {}>", rg_attrs.join(" "));
            for col in &rg.columns {
                let _ = writeln!(out, "      <RecordGroupColumn Name=\"{}\" ColumnDataType=\"Character\"/>", escape_attr(col));
            }
            out.push_str("    </RecordGroup>\n");
        }

        for lov in &module.lovs {
            let mut l_attrs = vec![format!("Name=\"{}\"", escape_attr(&lov.name))];
            if let Some(rg) = &lov.record_group_name {
                l_attrs.push(format!("RecordGroupName=\"{}\"", escape_attr(rg)));
            }
            let _ = writeln!(out, "    <LOV {}>", l_attrs.join(" "));
            for col_map in &lov.column_mappings {
                let _ = writeln!(out, "      <LOVColumnMapping Name=\"{}\"/>", escape_attr(col_map));
            }
            out.push_str("    </LOV>\n");
        }

        // Module Parameters
        for param in &module.parameters {
            let mut p_attrs = vec![format!("Name=\"{}\"", escape_attr(&param.name))];
            if let Some(dt) = &param.data_type {
                p_attrs.push(format!("ParameterDataType=\"{}\"", escape_attr(dt)));
            }
            if let Some(dv) = &param.default_value {
                p_attrs.push(format!("ParameterInitializeValue=\"{}\"", escape_attr(dv)));
            }
            let _ = writeln!(out, "    <ModuleParameter {}/>", p_attrs.join(" "));
        }

        // Program Units
        for pu in &module.program_units {
            let mut pu_attrs = vec![format!("Name=\"{}\"", escape_attr(&pu.name))];
            let p_type_str = match pu.unit_type {
                ProgramUnitType::Procedure => "Procedure",
                ProgramUnitType::Function => "Function",
                ProgramUnitType::PackageSpec => "Package Spec",
                ProgramUnitType::PackageBody => "Package Body",
                _ => "Unknown",
            };
            pu_attrs.push(format!("ProgramUnitType=\"{}\"", p_type_str));
            let text = pu
                .source_text
                .as_deref()
                .unwrap_or(&pu.first_line);
            pu_attrs.push(format!("ProgramUnitText=\"{}\"", escape_attr(text)));
            let _ = writeln!(out, "    <ProgramUnit {}/>", pu_attrs.join(" "));
        }

        // Property Classes
        for pc_name in &module.property_classes {
            if let Some(pc) = module.property_class_details.iter().find(|d| d.name.eq_ignore_ascii_case(pc_name)) {
                let mut pc_attrs = vec![format!("Name=\"{}\"", escape_attr(&pc.name))];
                if let Some(sc) = pc.smart_class {
                    pc_attrs.push(format!("SmartClass=\"{}\"", sc));
                }
                append_extra_attrs(&mut pc_attrs, &pc.extra_attrs);
                if pc.triggers.is_empty() {
                    let _ = writeln!(out, "    <PropertyClass {}/>", pc_attrs.join(" "));
                } else {
                    let _ = writeln!(out, "    <PropertyClass {}>", pc_attrs.join(" "));
                    for trig in &pc.triggers {
                        let text = trig.source_text.as_deref().unwrap_or(&trig.first_line);
                        let _ = writeln!(
                            out,
                            "      <Trigger Name=\"{}\" TriggerText=\"{}\"/>",
                            escape_attr(&trig.name),
                            escape_attr(text)
                        );
                    }
                    out.push_str("    </PropertyClass>\n");
                }
            } else {
                let _ = writeln!(out, "    <PropertyClass Name=\"{}\"/>", escape_attr(pc_name));
            }
        }

        // Visual Attributes
        for va in &module.visual_attributes {
            let _ = writeln!(out, "    <VisualAttribute Name=\"{}\"/>", escape_attr(va));
        }

        // Editors
        for ed in &module.editors {
            let _ = writeln!(out, "    <Editor Name=\"{}\"/>", escape_attr(ed));
        }

        // Object Groups
        for og in &module.object_groups {
            let mut og_attrs = vec![format!("Name=\"{}\"", escape_attr(&og.name))];
            append_extra_attrs(&mut og_attrs, &og.extra_attrs);
            if og.children.is_empty() {
                let _ = writeln!(out, "    <ObjectGroup {}/>", og_attrs.join(" "));
            } else {
                let _ = writeln!(out, "    <ObjectGroup {}>", og_attrs.join(" "));
                for ch in &og.children {
                    let mut ch_attrs = vec![format!("Name=\"{}\"", escape_attr(&ch.name))];
                    if let Some(sub) = ch.subclass_sub_object {
                        ch_attrs.push(format!("SubclassSubObject=\"{}\"", sub));
                    }
                    if let Some(ot) = &ch.object_type {
                        ch_attrs.push(format!("Type=\"{}\"", escape_attr(ot)));
                    }
                    if let Some(put) = &ch.program_unit_type {
                        ch_attrs.push(format!("ProgramUnitType=\"{}\"", escape_attr(put)));
                    }
                    append_extra_attrs(&mut ch_attrs, &ch.extra_attrs);
                    let _ = writeln!(out, "      <ObjectGroupChild {}/>", ch_attrs.join(" "));
                }
                out.push_str("    </ObjectGroup>\n");
            }
        }

        // Form Reports
        for rep in &module.form_reports {
            let mut r_attrs = vec![format!("Name=\"{}\"", escape_attr(&rep.name))];
            if let Some(v) = &rep.filename {
                r_attrs.push(format!("Filename=\"{}\"", escape_attr(v)));
            }
            if let Some(v) = &rep.execution_mode {
                r_attrs.push(format!("ExecutionMode=\"{}\"", escape_attr(v)));
            }
            if let Some(v) = &rep.comm_mode {
                r_attrs.push(format!("CommMode=\"{}\"", escape_attr(v)));
            }
            if let Some(v) = &rep.destype {
                r_attrs.push(format!("Destype=\"{}\"", escape_attr(v)));
            }
            if let Some(v) = &rep.desformat {
                r_attrs.push(format!("Desformat=\"{}\"", escape_attr(v)));
            }
            if let Some(v) = &rep.desname {
                r_attrs.push(format!("Desname=\"{}\"", escape_attr(v)));
            }
            append_extra_attrs(&mut r_attrs, &rep.extra_attrs);
            let _ = writeln!(out, "    <Report {}/>", r_attrs.join(" "));
        }

        // Form-level Triggers
        for trig in &module.triggers {
            if trig.level == TriggerLevel::Form && trig.block_name.is_none() && trig.item_name.is_none() {
                let text = trig
                    .source_text
                    .as_deref()
                    .unwrap_or(&trig.first_line);
                let _ = writeln!(
                    out,
                    "    <Trigger Name=\"{}\" TriggerText=\"{}\"/>",
                    escape_attr(&trig.name),
                    escape_attr(text)
                );
            }
        }

        for extra in &module.extra_children {
            write_raw_xml_node(out, extra, 4);
        }

        out.push_str("  </FormModule>\n");
    }

    fn write_block(out: &mut String, block: &BlockInfo, module: &ModuleIndex) {
        let mut b_attrs = vec![format!("Name=\"{}\"", escape_attr(&block.name))];

        if let Some(src) = &block.query_data_source_name {
            b_attrs.push(format!("QueryDataSourceName=\"{}\"", escape_attr(src)));
        }
        if let Some(pc) = &block.property_class {
            b_attrs.push(format!("ParentModule=\"{}\"", escape_attr(&module.key.name)));
            b_attrs.push("ParentModuleType=\"12\"".to_string());
            b_attrs.push(format!("ParentName=\"{}\"", escape_attr(pc)));
            b_attrs.push("ParentType=\"29\"".to_string());
        }
        if block.records_displayed > 1 {
            b_attrs.push(format!("RecordsDisplayCount=\"{}\"", block.records_displayed));
        }
        if block.record_orientation != "Vertical" && !block.record_orientation.is_empty() {
            b_attrs.push(format!("RecordOrientation=\"{}\"", escape_attr(&block.record_orientation)));
        }
        if let Some(ns) = &block.navigation_style {
            b_attrs.push(format!("NavigationStyle=\"{}\"", escape_attr(ns)));
        }
        if let Some(nb) = &block.next_navigation_block_name {
            b_attrs.push(format!("NextNavigationBlockName=\"{}\"", escape_attr(nb)));
        }
        if let Some(pb) = &block.previous_navigation_block_name {
            b_attrs.push(format!("PreviousNavigationBlockName=\"{}\"", escape_attr(pb)));
        }
        if let Some(ss) = block.show_scrollbar {
            b_attrs.push(format!("ShowScrollbar=\"{}\"", ss));
        }
        if let Some(va) = &block.visual_attribute {
            b_attrs.push(format!("VisualAttributeName=\"{}\"", escape_attr(va)));
        }
        if let Some(dt) = &block.dml_data_type {
            b_attrs.push(format!("DMLDataType=\"{}\"", escape_attr(dt)));
        }
        if let Some(qt) = &block.query_data_source_type {
            b_attrs.push(format!("QueryDataSourceType=\"{}\"", escape_attr(qt)));
        }

        if let Some(dml) = &block.dml {
            if let Some(where_c) = &dml.where_clause {
                b_attrs.push(format!("WhereClause=\"{}\"", escape_attr(where_c)));
            }
            if let Some(order_c) = &dml.order_by_clause {
                b_attrs.push(format!("OrderByClause=\"{}\"", escape_attr(order_c)));
            }
            if let Some(target) = &dml.dml_data_target_name {
                b_attrs.push(format!("DMLDataTargetName=\"{}\"", escape_attr(target)));
            }
            if let Some(db) = dml.database_block {
                b_attrs.push(format!("DatabaseBlock=\"{}\"", db));
            }
            if let Some(ins) = dml.insert_allowed {
                b_attrs.push(format!("InsertAllowed=\"{}\"", ins));
            }
            if let Some(upd) = dml.update_allowed {
                b_attrs.push(format!("UpdateAllowed=\"{}\"", upd));
            }
            if let Some(del) = dml.delete_allowed {
                b_attrs.push(format!("DeleteAllowed=\"{}\"", del));
            }
            if let Some(qry) = dml.query_allowed {
                b_attrs.push(format!("QueryAllowed=\"{}\"", qry));
            }
            if let Some(lock) = &dml.lock_mode {
                b_attrs.push(format!("LockMode=\"{}\"", escape_attr(lock)));
            }
        }
        append_extra_attrs(&mut b_attrs, &block.extra_attrs);

        let _ = writeln!(out, "    <Block {}>", b_attrs.join(" "));

        // Items
        for item in &block.items {
            Self::write_item(out, &block.name, item, module);
        }

        // Block-level triggers (block_name matches, item_name is None)
        for trig in &module.triggers {
            let is_block_trig = trig
                .block_name
                .as_deref()
                .map(|b| b.eq_ignore_ascii_case(&block.name))
                .unwrap_or(false)
                && trig.item_name.is_none();

            if is_block_trig {
                let text = trig
                    .source_text
                    .as_deref()
                    .unwrap_or(&trig.first_line);
                let _ = writeln!(
                    out,
                    "      <Trigger Name=\"{}\" TriggerText=\"{}\"/>",
                    escape_attr(&trig.name),
                    escape_attr(text)
                );
            }
        }

        // Relations
        for rel in &block.relations {
            let mut r_attrs = vec![format!("Name=\"{}\"", escape_attr(&rel.name))];
            if let Some(det) = &rel.detail_block {
                r_attrs.push(format!("DetailBlock=\"{}\"", escape_attr(det)));
            }
            if let Some(join) = &rel.join_condition {
                r_attrs.push(format!("JoinCondition=\"{}\"", escape_attr(join)));
            }
            if let Some(del) = &rel.delete_record {
                r_attrs.push(format!("DeleteRecord=\"{}\"", escape_attr(del)));
            }
            if let Some(pmo) = rel.prevent_masterless_operations {
                r_attrs.push(format!("PreventMasterlessOperations=\"{}\"", pmo));
            }
            if let Some(aq) = rel.auto_query {
                r_attrs.push(format!("AutoQuery=\"{}\"", aq));
            }
            if let Some(rtype) = &rel.relation_type {
                r_attrs.push(format!("RelationType=\"{}\"", escape_attr(rtype)));
            }
            let _ = writeln!(out, "      <Relation {}/>", r_attrs.join(" "));
        }

        for extra in &block.extra_children {
            write_raw_xml_node(out, extra, 6);
        }

        out.push_str("    </Block>\n");
    }

    fn write_item(out: &mut String, block_name: &str, item: &ItemInfo, module: &ModuleIndex) {
        let mut i_attrs = vec![format!("Name=\"{}\"", escape_attr(&item.name))];

        if let Some(itype) = &item.item_type {
            i_attrs.push(format!("ItemType=\"{}\"", escape_attr(itype)));
        }
        if let Some(dtype) = &item.data_type {
            i_attrs.push(format!("DataType=\"{}\"", escape_attr(dtype)));
        }
        if let Some(col) = &item.column_name {
            i_attrs.push(format!("ColumnName=\"{}\"", escape_attr(col)));
        }
        if let Some(canvas) = &item.canvas_name {
            i_attrs.push(format!("CanvasName=\"{}\"", escape_attr(canvas)));
        }
        if let Some(tab) = &item.tab_page_name {
            i_attrs.push(format!("TabPageName=\"{}\"", escape_attr(tab)));
        }
        if let Some(prompt) = &item.prompt {
            i_attrs.push(format!("Prompt=\"{}\"", escape_attr(prompt)));
        }
        if let Some(label) = &item.label {
            i_attrs.push(format!("Label=\"{}\"", escape_attr(label)));
        }
        if let Some(lov) = &item.lov_name {
            i_attrs.push(format!("LovName=\"{}\"", escape_attr(lov)));
        }
        if let Some(pc) = &item.property_class {
            i_attrs.push(format!("ParentModule=\"{}\"", escape_attr(&module.key.name)));
            i_attrs.push("ParentModuleType=\"12\"".to_string());
            i_attrs.push(format!("ParentName=\"{}\"", escape_attr(pc)));
            i_attrs.push("ParentType=\"29\"".to_string());
        }

        // Coordinates & Dimensions
        if item.x_position != 0 {
            i_attrs.push(format!("XPosition=\"{}\"", item.x_position));
        }
        if item.y_position != 0 {
            i_attrs.push(format!("YPosition=\"{}\"", item.y_position));
        }
        if let Some(w) = item.width {
            i_attrs.push(format!("Width=\"{}\"", w));
        }
        if let Some(h) = item.height {
            i_attrs.push(format!("Height=\"{}\"", h));
        }
        if item.visible == Some(false) {
            i_attrs.push("Visible=\"false\"".to_string());
        }
        if !item.enabled {
            i_attrs.push("Enabled=\"false\"".to_string());
        }
        if let Some(bc) = &item.back_color {
            i_attrs.push(format!("BackColor=\"{}\"", escape_attr(bc)));
        }
        if let Some(fc) = &item.fore_color {
            i_attrs.push(format!("ForegroundColor=\"{}\"", escape_attr(fc)));
        }
        if let Some(hint) = &item.hint {
            i_attrs.push(format!("Hint=\"{}\"", escape_attr(hint)));
        }
        if let Some(ak) = &item.access_key {
            i_attrs.push(format!("AccessKey=\"{}\"", escape_attr(ak)));
        }
        if let Some(ah) = item.auto_hint {
            i_attrs.push(format!("AutoHint=\"{}\"", ah));
        }
        if let Some(pi) = &item.previous_navigation_item_name {
            i_attrs.push(format!("PreviousNavigationItemName=\"{}\"", escape_attr(pi)));
        }
        if let Some(kn) = item.keyboard_navigable {
            i_attrs.push(format!("KeyboardNavigable=\"{}\"", kn));
        }
        if let Some(mn) = item.mouse_navigate {
            i_attrs.push(format!("MouseNavigate=\"{}\"", mn));
        }
        if item.items_display > 1 {
            i_attrs.push(format!("ItemsDisplay=\"{}\"", item.items_display));
        }

        // Item DML properties
        if let Some(dml) = &item.dml {
            if let Some(db_item) = dml.database_item {
                i_attrs.push(format!("DatabaseItem=\"{}\"", db_item));
            }
            if let Some(ins) = dml.insert_allowed {
                i_attrs.push(format!("InsertAllowed=\"{}\"", ins));
            }
            if let Some(upd) = dml.update_allowed {
                i_attrs.push(format!("UpdateAllowed=\"{}\"", upd));
            }
            if let Some(qry) = dml.query_allowed {
                i_attrs.push(format!("QueryAllowed=\"{}\"", qry));
            }
            if let Some(req) = dml.required {
                i_attrs.push(format!("Required=\"{}\"", req));
            }
            if let Some(pk) = dml.primary_key {
                i_attrs.push(format!("PrimaryKey=\"{}\"", pk));
            }
            if let Some(max_len) = dml.maximum_length {
                i_attrs.push(format!("MaximumLength=\"{}\"", max_len));
            }
        }
        append_extra_attrs(&mut i_attrs, &item.extra_attrs);

        // Find item-level triggers
        let item_trigs: Vec<&TriggerInfo> = module
            .triggers
            .iter()
            .filter(|t| {
                t.block_name
                    .as_deref()
                    .map(|b| b.eq_ignore_ascii_case(block_name))
                    .unwrap_or(false)
                    && t.item_name
                        .as_deref()
                        .map(|i| i.eq_ignore_ascii_case(&item.name))
                        .unwrap_or(false)
            })
            .collect();

        if item_trigs.is_empty() && item.radio_buttons.is_empty() && item.list_elements.is_empty() && item.extra_children.is_empty() {
            let _ = writeln!(out, "      <Item {}/>", i_attrs.join(" "));
        } else {
            let _ = writeln!(out, "      <Item {}>", i_attrs.join(" "));
            for rb in &item.radio_buttons {
                let mut r_attrs = vec![format!("Name=\"{}\"", escape_attr(&rb.name))];
                if let Some(lbl) = &rb.label {
                    r_attrs.push(format!("Label=\"{}\"", escape_attr(lbl)));
                }
                if let Some(val) = &rb.value {
                    r_attrs.push(format!("RadioButtonValue=\"{}\"", escape_attr(val)));
                }
                if rb.x_position != 0 {
                    r_attrs.push(format!("XPosition=\"{}\"", rb.x_position));
                }
                if rb.y_position != 0 {
                    r_attrs.push(format!("YPosition=\"{}\"", rb.y_position));
                }
                if let Some(w) = rb.width {
                    r_attrs.push(format!("Width=\"{}\"", w));
                }
                if let Some(h) = rb.height {
                    r_attrs.push(format!("Height=\"{}\"", h));
                }
                let _ = writeln!(out, "        <RadioButton {}/>", r_attrs.join(" "));
            }
            for le in &item.list_elements {
                let mut le_attrs = vec![format!("Name=\"{}\"", escape_attr(&le.name))];
                if let Some(val) = &le.value {
                    le_attrs.push(format!("Value=\"{}\"", escape_attr(val)));
                }
                let _ = writeln!(out, "        <ListItemElement {}/>", le_attrs.join(" "));
            }
            for trig in item_trigs {
                let text = trig
                    .source_text
                    .as_deref()
                    .unwrap_or(&trig.first_line);
                let _ = writeln!(
                    out,
                    "        <Trigger Name=\"{}\" TriggerText=\"{}\"/>",
                    escape_attr(&trig.name),
                    escape_attr(text)
                );
            }
            for extra in &item.extra_children {
                write_raw_xml_node(out, extra, 8);
            }
            out.push_str("      </Item>\n");
        }
    }

    fn write_canvas(out: &mut String, canvas: &CanvasInfo, module: &ModuleIndex) {
        let mut c_attrs = vec![format!("Name=\"{}\"", escape_attr(&canvas.name))];
        if let Some(ctype) = &canvas.canvas_type {
            c_attrs.push(format!("CanvasType=\"{}\"", escape_attr(ctype)));
        }
        if let Some(wname) = &canvas.window_name {
            c_attrs.push(format!("WindowName=\"{}\"", escape_attr(wname)));
        }
        c_attrs.push(format!("Width=\"{}\"", canvas.width));
        c_attrs.push(format!("Height=\"{}\"", canvas.height));
        if let Some(vw) = canvas.viewport_width {
            c_attrs.push(format!("ViewportWidth=\"{}\"", vw));
        }
        if let Some(vh) = canvas.viewport_height {
            c_attrs.push(format!("ViewportHeight=\"{}\"", vh));
        }

        // Graphics / Frames on this canvas
        let canvas_graphics: Vec<&GraphicInfo> = module
            .graphics
            .iter()
            .filter(|g| g.canvas_name.as_deref().map(|c| c.eq_ignore_ascii_case(&canvas.name)).unwrap_or(false))
            .collect();

        if canvas_graphics.is_empty() && canvas.tab_pages.is_empty() {
            let _ = writeln!(out, "    <Canvas {}/>", c_attrs.join(" "));
        } else {
            let _ = writeln!(out, "    <Canvas {}>", c_attrs.join(" "));
            for tp in &canvas.tab_pages {
                let mut t_attrs = vec![format!("Name=\"{}\"", escape_attr(&tp.name))];
                if let Some(lbl) = &tp.label {
                    t_attrs.push(format!("Label=\"{}\"", escape_attr(lbl)));
                }
                let _ = writeln!(out, "      <TabPage {}/>", t_attrs.join(" "));
            }
            for g in canvas_graphics {
                let mut g_attrs = vec![format!("Name=\"{}\"", escape_attr(&g.name))];
                if !g.graphics_type.is_empty() {
                    g_attrs.push(format!("GraphicsType=\"{}\"", escape_attr(&g.graphics_type)));
                }
                if let Some(ftitle) = &g.frame_title {
                    g_attrs.push(format!("FrameTitle=\"{}\"", escape_attr(ftitle)));
                }
                if g.x_position != 0 {
                    g_attrs.push(format!("XPosition=\"{}\"", g.x_position));
                }
                if g.y_position != 0 {
                    g_attrs.push(format!("YPosition=\"{}\"", g.y_position));
                }
                if g.width != 0 {
                    g_attrs.push(format!("Width=\"{}\"", g.width));
                }
                if let Some(h) = g.height.checked_sub(0).filter(|&v| v != 0) {
                    g_attrs.push(format!("Height=\"{}\"", h));
                }
                append_extra_attrs(&mut g_attrs, &g.extra_attrs);
                if g.points.is_empty() && g.compound_texts.is_empty() {
                    let _ = writeln!(out, "      <Graphics {}/>", g_attrs.join(" "));
                } else {
                    let _ = writeln!(out, "      <Graphics {}>", g_attrs.join(" "));
                    for pt in &g.points {
                        let _ = writeln!(
                            out,
                            "        <Point Name=\"{}\" XPosition=\"{}\" YPosition=\"{}\"/>",
                            escape_attr(&pt.name),
                            pt.x_position,
                            pt.y_position
                        );
                    }
                    for ct in &g.compound_texts {
                        if ct.text_segments.is_empty() {
                            let _ = writeln!(out, "        <CompoundText Name=\"{}\"/>", escape_attr(&ct.name));
                        } else {
                            let _ = writeln!(out, "        <CompoundText Name=\"{}\">", escape_attr(&ct.name));
                            for ts in &ct.text_segments {
                                let mut ts_attrs = vec![format!("Name=\"{}\"", escape_attr(&ts.name))];
                                if let Some(t) = &ts.text {
                                    ts_attrs.push(format!("Text=\"{}\"", escape_attr(t)));
                                }
                                if let Some(fn_) = &ts.font_name {
                                    ts_attrs.push(format!("FontName=\"{}\"", escape_attr(fn_)));
                                }
                                if let Some(fs) = ts.font_size {
                                    ts_attrs.push(format!("FontSize=\"{}\"", fs));
                                }
                                if let Some(fw) = &ts.font_weight {
                                    ts_attrs.push(format!("FontWeight=\"{}\"", escape_attr(fw)));
                                }
                                if let Some(fst) = &ts.font_style {
                                    ts_attrs.push(format!("FontStyle=\"{}\"", escape_attr(fst)));
                                }
                                if let Some(fsp) = &ts.font_spacing {
                                    ts_attrs.push(format!("FontSpacing=\"{}\"", escape_attr(fsp)));
                                }
                                if let Some(fg) = &ts.foreground_color {
                                    ts_attrs.push(format!("ForegroundColor=\"{}\"", escape_attr(fg)));
                                }
                                append_extra_attrs(&mut ts_attrs, &ts.extra_attrs);
                                let _ = writeln!(out, "          <TextSegment {}/>", ts_attrs.join(" "));
                            }
                            out.push_str("        </CompoundText>\n");
                        }
                    }
                    out.push_str("      </Graphics>\n");
                }
            }
            out.push_str("    </Canvas>\n");
        }
    }

    fn write_menu_module(out: &mut String, module: &ModuleIndex) {
        let menu_name = &module.key.name;
        let _ = writeln!(out, "  <MenuModule Name=\"{}\">", escape_attr(menu_name));
        for menu in &module.menus {
            let _ = writeln!(out, "    <Menu Name=\"{}\">", escape_attr(&menu.name));
            for item in &menu.items {
                let mut mi_attrs = vec![format!("Name=\"{}\"", escape_attr(&item.name))];
                if let Some(lbl) = &item.label {
                    mi_attrs.push(format!("Label=\"{}\"", escape_attr(lbl)));
                }
                if let Some(cmd) = &item.command_type {
                    mi_attrs.push(format!("CommandType=\"{}\"", escape_attr(cmd)));
                }
                if let Some(mit) = &item.menu_item_type {
                    mi_attrs.push(format!("MenuItemType=\"{}\"", escape_attr(mit)));
                }
                if let Some(sub) = &item.sub_menu_name {
                    mi_attrs.push(format!("SubMenuName=\"{}\"", escape_attr(sub)));
                }
                let _ = writeln!(out, "      <MenuItem {}/>", mi_attrs.join(" "));
            }
            out.push_str("    </Menu>\n");
        }
        out.push_str("  </MenuModule>\n");
    }

    fn write_object_library(out: &mut String, module: &ModuleIndex) {
        let olb_name = &module.key.name;
        let _ = writeln!(out, "  <ObjectLibrary Name=\"{}\">", escape_attr(olb_name));
        for tab in &module.object_library_tabs {
            let _ = writeln!(out, "    <ObjectLibraryTab Name=\"{}\">", escape_attr(&tab.name));
            for entry in &tab.entries {
                let mut e_attrs = vec![format!("Name=\"{}\"", escape_attr(&entry.name))];
                if let Some(otype) = &entry.object_type {
                    e_attrs.push(format!("ObjectType=\"{}\"", escape_attr(otype)));
                }
                let _ = writeln!(out, "      <ObjectLibraryEntry {}/>", e_attrs.join(" "));
            }
            out.push_str("    </ObjectLibraryTab>\n");
        }
        out.push_str("  </ObjectLibrary>\n");
    }
}

/// Appends any unmodeled XML attributes that were not already explicitly emitted.
fn append_extra_attrs(attrs: &mut Vec<String>, extra: &BTreeMap<String, String>) {
    for (k, v) in extra {
        let already = attrs.iter().any(|existing| {
            existing
                .split_once('=')
                .map(|(ek, _)| ek.eq_ignore_ascii_case(k))
                .unwrap_or(false)
        });
        if !already {
            attrs.push(format!("{}=\"{}\"", k, escape_attr(v)));
        }
    }
}

/// Recursively serializes an unmodeled Forms XML element and its children.
fn write_raw_xml_node(out: &mut String, node: &RawXmlNode, indent: usize) {
    let pad = " ".repeat(indent);
    let mut attrs_vec = Vec::with_capacity(node.attrs.len());
    for (k, v) in &node.attrs {
        attrs_vec.push(format!("{}=\"{}\"", k, escape_attr(v)));
    }
    let attr_str = if attrs_vec.is_empty() {
        String::new()
    } else {
        format!(" {}", attrs_vec.join(" "))
    };
    if node.children.is_empty() {
        let _ = writeln!(out, "{}<{}{}/>", pad, node.tag, attr_str);
    } else {
        let _ = writeln!(out, "{}<{}{}>", pad, node.tag, attr_str);
        for child in &node.children {
            write_raw_xml_node(out, child, indent + 2);
        }
        let _ = writeln!(out, "{}</{}>", pad, node.tag);
    }
}

/// Returns true for characters allowed in XML 1.0 documents.
fn is_valid_xml_char(ch: char) -> bool {
    let code = ch as u32;
    matches!(code, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// Escapes standard XML characters for attribute strings and strips illegal XML 1.0 control bytes.
pub(crate) fn escape_attr(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\n' => escaped.push_str("&#10;"),
            '\r' => escaped.push_str("&#13;"),
            '\t' => escaped.push('\t'),
            c if is_valid_xml_char(c) => escaped.push(c),
            _ => {}
        }
    }
    escaped
}

/// `forms-14c.xsd` declares `<Module version>` as `xs:long`; replace non-numeric strings (e.g. "6i (ROS.60050)").
pub(crate) fn normalize_module_version(forms_version: Option<&str>) -> &str {
    forms_version
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or("122010400")
}

#[cfg(test)]
mod tests {
    use super::{append_extra_attrs, escape_attr, normalize_module_version};
    use std::collections::BTreeMap;

    #[test]
    fn append_extra_attrs_skips_case_insensitive_duplicates() {
        let mut attrs = vec![
            "Name=\"B1\"".to_string(),
            "ParentModule=\"M1\"".to_string(),
            "ParentName=\"PC1\"".to_string(),
        ];
        let mut extra = BTreeMap::new();
        extra.insert("name".to_string(), "B1".to_string());
        extra.insert("ParentName".to_string(), "PC1".to_string());
        extra.insert("DirtyInfo".to_string(), "true".to_string());
        append_extra_attrs(&mut attrs, &extra);
        assert_eq!(attrs.len(), 4);
        assert!(attrs.iter().any(|a| a == "DirtyInfo=\"true\""));
    }

    #[test]
    fn escape_attr_strips_illegal_xml_control_chars() {
        let input = "BEGIN\x00\x01\x08\x0B\x0C\x1F\t\nEND";
        assert_eq!(escape_attr(input), "BEGIN\t&#10;END");
    }

    #[test]
    fn normalize_module_version_requires_numeric_xsd_long() {
        assert_eq!(normalize_module_version(None), "122010400");
        assert_eq!(normalize_module_version(Some("6i (ROS.60050)")), "122010400");
        assert_eq!(normalize_module_version(Some("12.2.1.19.0")), "122010400");
        assert_eq!(normalize_module_version(Some("140102000")), "140102000");
    }
}
