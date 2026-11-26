//! Purpose: Streaming Forms2XML parser into ModuleIndex.
//! Key Input Dependencies: quick_xml, oracle_forms_core
//! Index: Frame, BlockBuilder, ItemBuilder, CanvasBuilder, FormsXmlParser, parse, handle_start_element, handle_end_element, collect_attrs, get_attr, get_bool_attr, get_int_attr, item_dml, visual_style, item_geometry, block_dml, local_parent_name, inheritance_of, join_path, owner_path_of, parses_graphics_fill_pattern_multiline_prompts_items_display_and_radio_buttons

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use quick_xml::events::BytesStart;
use quick_xml::reader::Reader;
use oracle_forms_core::error::{FormsError, Result};
use oracle_forms_core::model::*;
use oracle_forms_core::sidecars::{
    cache_relative, decode_double_escaped, first_code_line, line_count_of, unescape_xml_entities, PlsqlSidecars,
};
use crate::pld::chrono_iso_now;

struct Frame {
    element: String,
    name: Option<String>,
    start_line: usize,
    owner_path: Option<String>,
    inherited: Option<InheritanceRef>,
}

struct BlockBuilder {
    name: String,
    query_data_source_name: Option<String>,
    property_class: Option<String>,
    inherited: Option<InheritanceRef>,
    dml: Option<BlockDml>,
    records_displayed: i32,
    distance_between_records: i32,
    record_orientation: String,
    items: Vec<ItemInfo>,
    trigger_names: Vec<String>,
    relations: Vec<RelationInfo>,
    data_source_column_count: i32,
    navigation_style: Option<String>,
    next_navigation_block_name: Option<String>,
    previous_navigation_block_name: Option<String>,
    show_scrollbar: Option<bool>,
    visual_attribute: Option<String>,
    dml_data_type: Option<String>,
    query_data_source_type: Option<String>,
    extra_attrs: BTreeMap<String, String>,
    extra_children: Vec<RawXmlNode>,
}

struct ItemBuilder {
    name: String,
    item_type: Option<String>,
    data_type: Option<String>,
    column_name: Option<String>,
    canvas_name: Option<String>,
    prompt: Option<String>,
    property_class: Option<String>,
    visible: Option<bool>,
    required: Option<bool>,
    lov_name: Option<String>,
    inherited: Option<InheritanceRef>,
    dml: Option<ItemDml>,
    width: Option<i32>,
    height: Option<i32>,
    x_position: i32,
    y_position: i32,
    tab_page_name: Option<String>,
    label: Option<String>,
    prompt_attachment_edge: String,
    prompt_attachment_offset: i32,
    prompt_align: String,
    prompt_align_offset: i32,
    prompt_edge_set: bool,
    prompt_offset_set: bool,
    prompt_align_set: bool,
    items_display: i32,
    distance_between_records: i32,
    back_color: Option<String>,
    fore_color: Option<String>,
    fill_pattern: Option<String>,
    bevel: Option<String>,
    font_name: Option<String>,
    font_size: Option<i32>,
    font_weight: Option<String>,
    font_style: Option<String>,
    visual_attribute: Option<String>,
    prompt_fore_color: Option<String>,
    prompt_font_name: Option<String>,
    prompt_font_size: Option<i32>,
    prompt_font_weight: Option<String>,
    radio_buttons: Vec<RadioButtonInfo>,
    enabled: bool,
    canvas_visible: bool,
    trigger_names: Vec<String>,
    hint: Option<String>,
    access_key: Option<String>,
    auto_hint: Option<bool>,
    previous_navigation_item_name: Option<String>,
    keyboard_navigable: Option<bool>,
    mouse_navigate: Option<bool>,
    extra_attrs: BTreeMap<String, String>,
    extra_children: Vec<RawXmlNode>,
}

struct CanvasBuilder {
    name: String,
    canvas_type: Option<String>,
    window_name: Option<String>,
    property_class: Option<String>,
    raise_on_enter: Option<bool>,
    width: i32,
    height: i32,
    viewport_width: Option<i32>,
    viewport_height: Option<i32>,
    viewport_x_position: i32,
    viewport_y_position: i32,
    viewport_x_position_on_canvas: Option<i32>,
    viewport_y_position_on_canvas: Option<i32>,
    back_color: Option<String>,
    visual_attribute: Option<String>,
    inherited: Option<InheritanceRef>,
    tab_pages: Vec<TabPageInfo>,
}

pub struct FormsXmlParser;

impl FormsXmlParser {
    pub fn parse(key: &ModuleKey, converted_file: &Path, module_cache_dir: &Path) -> Result<ModuleIndex> {
        let content_bytes = fs::read(converted_file)?;
        let xml_path = cache_relative(converted_file, module_cache_dir);
        let mut sidecars = PlsqlSidecars::new(module_cache_dir);

        // Precompute line start byte offsets
        let mut line_starts = vec![0usize];
        for (idx, &b) in content_bytes.iter().enumerate() {
            if b == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        let byte_to_line = |pos: usize| -> usize {
            match line_starts.binary_search(&pos) {
                Ok(idx) => idx + 1,
                Err(idx) => idx,
            }
        };

        let mut forms_version: Option<String> = None;
        let mut module_name: String = key.name.clone();
        let mut coordinate_info = CoordinateInfo::default();
        let mut blocks: Vec<BlockInfo> = Vec::new();
        let mut triggers: Vec<TriggerInfo> = Vec::new();
        let mut program_units: Vec<ProgramUnitInfo> = Vec::new();
        let mut attached_libraries: Vec<AttachedLibraryInfo> = Vec::new();
        let mut lovs: Vec<LovInfo> = Vec::new();
        let mut record_groups: Vec<RecordGroupInfo> = Vec::new();
        let mut windows: Vec<WindowInfo> = Vec::new();
        let mut canvases: Vec<CanvasInfo> = Vec::new();
        let mut graphics: Vec<GraphicInfo> = Vec::new();
        let mut alerts: Vec<AlertInfo> = Vec::new();
        let mut parameters: Vec<ParameterInfo> = Vec::new();
        let mut visual_attributes: Vec<String> = Vec::new();
        let mut visual_attribute_details: Vec<VisualAttributeInfo> = Vec::new();
        let mut property_classes: Vec<String> = Vec::new();
        let mut property_class_details: Vec<PropertyClassInfo> = Vec::new();
        let mut editors: Vec<String> = Vec::new();
        let mut menu_module: Option<String> = None;
        let mut menus: Vec<MenuInfo> = Vec::new();
        let mut object_library_tabs: Vec<ObjectLibraryTabInfo> = Vec::new();
        let mut object_refs: Vec<ObjectRef> = Vec::new();
        let mut first_navigation_block_name: Option<String> = None;
        let mut runtime_comp: Option<String> = None;
        let mut module_title: Option<String> = None;
        let mut console_window: Option<String> = None;
        let mut object_groups: Vec<ObjectGroupInfo> = Vec::new();
        let mut form_reports: Vec<FormReportInfo> = Vec::new();
        let mut module_extra_attrs: BTreeMap<String, String> = BTreeMap::new();
        let mut module_extra_children: Vec<RawXmlNode> = Vec::new();
        let mut raw_node_stack: Vec<(usize, RawXmlNode)> = Vec::new();

        let mut current_block: Option<BlockBuilder> = None;
        let mut current_item: Option<ItemBuilder> = None;
        let mut current_canvas: Option<CanvasBuilder> = None;
        let mut current_tab_page_name: Option<String> = None;
        let mut current_lov: Option<LovInfo> = None;
        let mut current_record_group: Option<RecordGroupInfo> = None;
        let mut current_menu: Option<MenuInfo> = None;
        let mut current_tab: Option<ObjectLibraryTabInfo> = None;

        let mut stack: Vec<Frame> = Vec::new();

        let mut reader = Reader::from_reader(content_bytes.as_slice());
        reader.config_mut().trim_text(true);

        let mut buf = Vec::new();

        loop {
            let start_pos = reader.buffer_position() as usize;
            let event = match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(e) => e,
                Err(e) => return Err(FormsError::Xml(format!("XML parse error at byte {}: {:?}", start_pos, e))),
            };
            let end_pos = reader.buffer_position() as usize;
            let start_line = byte_to_line(start_pos);
            let end_line = byte_to_line(end_pos).max(start_line);

            match event {
                quick_xml::events::Event::Start(ref e) => {
                    Self::handle_start_element(
                        e,
                        start_line,
                        &mut stack,
                        &mut forms_version,
                        &mut module_name,
                        &mut coordinate_info,
                        &mut current_block,
                        &mut current_item,
                        &mut current_canvas,
                        &mut current_tab_page_name,
                        &mut graphics,
                        &mut triggers,
                        &mut program_units,
                        &mut attached_libraries,
                        &mut current_lov,
                        &mut current_record_group,
                        &mut windows,
                        &mut alerts,
                        &mut parameters,
                        &mut visual_attributes,
                        &mut visual_attribute_details,
                        &mut property_classes,
                        &mut property_class_details,
                        &mut editors,
                        &mut menu_module,
                        &mut current_menu,
                        &mut current_tab,
                        &mut first_navigation_block_name,
                        &mut runtime_comp,
                        &mut module_title,
                        &mut console_window,
                        &mut object_groups,
                        &mut form_reports,
                        &mut module_extra_attrs,
                        &mut raw_node_stack,
                        &mut sidecars,
                    )?;
                }
                quick_xml::events::Event::Empty(ref e) => {
                    Self::handle_start_element(
                        e,
                        start_line,
                        &mut stack,
                        &mut forms_version,
                        &mut module_name,
                        &mut coordinate_info,
                        &mut current_block,
                        &mut current_item,
                        &mut current_canvas,
                        &mut current_tab_page_name,
                        &mut graphics,
                        &mut triggers,
                        &mut program_units,
                        &mut attached_libraries,
                        &mut current_lov,
                        &mut current_record_group,
                        &mut windows,
                        &mut alerts,
                        &mut parameters,
                        &mut visual_attributes,
                        &mut visual_attribute_details,
                        &mut property_classes,
                        &mut property_class_details,
                        &mut editors,
                        &mut menu_module,
                        &mut current_menu,
                        &mut current_tab,
                        &mut first_navigation_block_name,
                        &mut runtime_comp,
                        &mut module_title,
                        &mut console_window,
                        &mut object_groups,
                        &mut form_reports,
                        &mut module_extra_attrs,
                        &mut raw_node_stack,
                        &mut sidecars,
                    )?;
                    Self::handle_end_element(
                        end_line,
                        &xml_path,
                        &mut stack,
                        &mut object_refs,
                        &mut triggers,
                        &mut program_units,
                        &mut current_block,
                        &mut current_item,
                        &mut blocks,
                        &mut current_canvas,
                        &mut canvases,
                        &mut current_tab_page_name,
                        &mut current_lov,
                        &mut lovs,
                        &mut current_record_group,
                        &mut record_groups,
                        &mut current_menu,
                        &mut menus,
                        &mut current_tab,
                        &mut object_library_tabs,
                        &mut module_extra_children,
                        &mut raw_node_stack,
                    );
                }
                quick_xml::events::Event::End(_) => {
                    Self::handle_end_element(
                        end_line,
                        &xml_path,
                        &mut stack,
                        &mut object_refs,
                        &mut triggers,
                        &mut program_units,
                        &mut current_block,
                        &mut current_item,
                        &mut blocks,
                        &mut current_canvas,
                        &mut canvases,
                        &mut current_tab_page_name,
                        &mut current_lov,
                        &mut lovs,
                        &mut current_record_group,
                        &mut record_groups,
                        &mut current_menu,
                        &mut menus,
                        &mut current_tab,
                        &mut object_library_tabs,
                        &mut module_extra_children,
                        &mut raw_node_stack,
                    );
                }
                _ => {}
            }
            buf.clear();
        }

        // Post-process property classes: only retain propertyClass if declared
        let declared: HashSet<String> = property_classes.iter().map(|s| s.to_ascii_uppercase()).collect();
        let classed_blocks = blocks.into_iter().map(|mut b| {
            b.property_class = b.property_class.filter(|s| declared.contains(&s.to_ascii_uppercase()));
            b.items = b.items.into_iter().map(|mut it| {
                it.property_class = it.property_class.filter(|s| declared.contains(&s.to_ascii_uppercase()));
                it
            }).collect();
            b
        }).collect();

        let classed_canvases = canvases.into_iter().map(|mut c| {
            c.property_class = c.property_class.filter(|s| declared.contains(&s.to_ascii_uppercase()));
            c
        }).collect();

        let classed_details = property_class_details.into_iter().map(|mut d| {
            d.property_class = d.property_class.filter(|s| declared.contains(&s.to_ascii_uppercase()));
            d
        }).collect();

        let now = chrono_iso_now();

        Ok(ModuleIndex {
            key: key.clone(),
            forms_version,
            index_version: oracle_forms_core::CURRENT_INDEX_VERSION,
            source_file: converted_file.to_string_lossy().to_string(),
            fingerprint: ModuleFingerprint::default(),
            converted_file: xml_path,
            parsed_at: now,
            coordinate: coordinate_info,
            blocks: classed_blocks,
            triggers,
            program_units,
            attached_libraries,
            lovs,
            record_groups,
            windows,
            canvases: classed_canvases,
            graphics,
            alerts,
            parameters,
            visual_attributes,
            visual_attribute_details,
            property_classes,
            property_class_details: classed_details,
            editors,
            menu_module,
            menus,
            object_library_tabs,
            object_refs,
            first_navigation_block_name,
            runtime_comp,
            title: module_title,
            console_window,
            object_groups,
            form_reports,
            extra_attrs: module_extra_attrs,
            extra_children: module_extra_children,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_start_element(
        e: &BytesStart,
        start_line: usize,
        stack: &mut Vec<Frame>,
        forms_version: &mut Option<String>,
        module_name: &mut String,
        coordinate_info: &mut CoordinateInfo,
        current_block: &mut Option<BlockBuilder>,
        current_item: &mut Option<ItemBuilder>,
        current_canvas: &mut Option<CanvasBuilder>,
        current_tab_page_name: &mut Option<String>,
        graphics: &mut Vec<GraphicInfo>,
        triggers: &mut Vec<TriggerInfo>,
        program_units: &mut Vec<ProgramUnitInfo>,
        attached_libraries: &mut Vec<AttachedLibraryInfo>,
        current_lov: &mut Option<LovInfo>,
        current_record_group: &mut Option<RecordGroupInfo>,
        windows: &mut Vec<WindowInfo>,
        alerts: &mut Vec<AlertInfo>,
        parameters: &mut Vec<ParameterInfo>,
        visual_attributes: &mut Vec<String>,
        visual_attribute_details: &mut Vec<VisualAttributeInfo>,
        property_classes: &mut Vec<String>,
        property_class_details: &mut Vec<PropertyClassInfo>,
        editors: &mut Vec<String>,
        menu_module: &mut Option<String>,
        current_menu: &mut Option<MenuInfo>,
        current_tab: &mut Option<ObjectLibraryTabInfo>,
        first_navigation_block_name: &mut Option<String>,
        runtime_comp: &mut Option<String>,
        module_title: &mut Option<String>,
        console_window: &mut Option<String>,
        object_groups: &mut Vec<ObjectGroupInfo>,
        form_reports: &mut Vec<FormReportInfo>,
        module_extra_attrs: &mut BTreeMap<String, String>,
        raw_node_stack: &mut Vec<(usize, RawXmlNode)>,
        sidecars: &mut PlsqlSidecars,
    ) -> Result<()> {
        let element = String::from_utf8_lossy(e.name().as_ref()).to_string();
        let name = get_attr(e, "Name");
        let parent_element = stack.last().map(|p| p.element.clone());
        let parent_inherited = stack.last().and_then(|p| p.inherited.clone());
        let owner_path = owner_path_of(stack);
        let inherited = inheritance_of(e, name.as_deref(), parent_inherited.as_ref(), module_name);

        stack.push(Frame {
            element: element.clone(),
            name: name.clone(),
            start_line,
            owner_path: owner_path.clone(),
            inherited: inherited.clone(),
        });

        if !raw_node_stack.is_empty() {
            raw_node_stack.push((
                stack.len(),
                RawXmlNode {
                    tag: element,
                    attrs: collect_attrs(e),
                    children: Vec::new(),
                },
            ));
            return Ok(());
        }

        match element.as_str() {
            "Module" => {
                *forms_version = get_attr(e, "version");
            }
            "FormModule" => {
                if let Some(ref n) = name {
                    *module_name = n.clone();
                }
                *menu_module = get_attr(e, "MenuModule");
                *first_navigation_block_name = get_attr(e, "FirstNavigationBlockName");
                *runtime_comp = get_attr(e, "RuntimeComp");
                *module_title = get_attr(e, "Title");
                *console_window = get_attr(e, "ConsoleWindow");
                *module_extra_attrs = collect_attrs(e);
            }
            "MenuModule" | "ObjectLibrary" => {
                if let Some(ref n) = name {
                    *module_name = n.clone();
                }
                *module_extra_attrs = collect_attrs(e);
            }
            "Coordinate" => {
                *coordinate_info = CoordinateInfo {
                    present: true,
                    coordinate_system: get_attr(e, "CoordinateSystem"),
                    real_unit: get_attr(e, "RealUnit").or_else(|| Some("Point".to_string())),
                    character_cell_width: get_int_attr(e, "CharacterCellWidth").unwrap_or(7),
                    character_cell_height: get_int_attr(e, "CharacterCellHeight").unwrap_or(14),
                    default_font_scaling: get_bool_attr(e, "DefaultFontScaling").unwrap_or(true),
                };
            }
            "Block" => {
                if matches!(parent_element.as_deref(), Some("FormModule") | Some("ObjectLibraryTab")) {
                    let source = get_attr(e, "QueryDataSourceName").map(|s| decode_double_escaped(&s));
                    let records_disp = get_int_attr(e, "RecordsDisplayCount")
                        .or_else(|| get_int_attr(e, "RecordsDisplayed"))
                        .unwrap_or(1);
                    let dist = get_int_attr(e, "DistanceBetweenRecords").unwrap_or(0);
                    let orient = get_attr(e, "RecordOrientation").unwrap_or_else(|| "Vertical".to_string());

                    *current_block = Some(BlockBuilder {
                        name: name.clone().unwrap_or_default(),
                        query_data_source_name: source.as_ref().map(|s| s.text.clone()),
                        property_class: local_parent_name(e, module_name),
                        inherited,
                        dml: block_dml(e, source.as_ref().map(|s| s.encoding == TextEncoding::Recovered).unwrap_or(false)),
                        records_displayed: records_disp,
                        distance_between_records: dist,
                        record_orientation: orient,
                        items: Vec::new(),
                        trigger_names: Vec::new(),
                        relations: Vec::new(),
                        data_source_column_count: 0,
                        navigation_style: get_attr(e, "NavigationStyle"),
                        next_navigation_block_name: get_attr(e, "NextNavigationBlockName"),
                        previous_navigation_block_name: get_attr(e, "PreviousNavigationBlockName"),
                        show_scrollbar: get_bool_attr(e, "ShowScrollbar"),
                        visual_attribute: get_attr(e, "VisualAttributeName"),
                        dml_data_type: get_attr(e, "DMLDataType"),
                        query_data_source_type: get_attr(e, "QueryDataSourceType"),
                        extra_attrs: collect_attrs(e),
                        extra_children: Vec::new(),
                    });
                }
            }
            "DataSourceColumn" => {
                if parent_element.as_deref() == Some("Block") {
                    if let Some(b) = current_block.as_mut() {
                        b.data_source_column_count += 1;
                    }
                    raw_node_stack.push((
                        stack.len(),
                        RawXmlNode {
                            tag: element.clone(),
                            attrs: collect_attrs(e),
                            children: Vec::new(),
                        },
                    ));
                }
            }
            "Relation" => {
                if parent_element.as_deref() == Some("Block") {
                    if let Some(b) = current_block.as_mut() {
                        let join = get_attr(e, "JoinCondition").map(|s| decode_double_escaped(&s));
                        b.relations.push(RelationInfo {
                            name: name.clone().unwrap_or_default(),
                            detail_block: get_attr(e, "DetailBlock"),
                            join_condition: join.as_ref().map(|j| j.text.clone()),
                            join_encoding: join.map(|j| j.encoding).unwrap_or_default(),
                            deferred: get_bool_attr(e, "Deferred"),
                            auto_query: get_bool_attr(e, "AutoQuery"),
                            prevent_masterless_operations: get_bool_attr(e, "PreventMasterlessOperations"),
                            delete_record: get_attr(e, "DeleteRecord"),
                            relation_type: get_attr(e, "RelationType"),
                            inherited,
                        });
                    }
                }
            }
            "Item" => {
                if parent_element.as_deref() == Some("Block") && current_block.is_some() {
                    let vis = get_bool_attr(e, "Visible").unwrap_or(true);
                    let enabled = get_bool_attr(e, "Enabled").unwrap_or(true);
                    let block_records = current_block.as_ref().map(|b| b.records_displayed).unwrap_or(1);
                    let block_dist = current_block.as_ref().map(|b| b.distance_between_records).unwrap_or(0);
                    let raw_items_disp = get_int_attr(e, "ItemsDisplay")
                        .or_else(|| get_int_attr(e, "ItemsDisplayed"));
                    let dist = get_int_attr(e, "DistanceBetweenRecords").unwrap_or(block_dist);

                    let raw_type = get_attr(e, "ItemType");
                    let parent_name = get_attr(e, "ParentName").unwrap_or_default();
                    let label = get_attr(e, "Label").map(|s| decode_double_escaped(&s).text);
                    let prompt = get_attr(e, "Prompt").map(|s| decode_double_escaped(&s).text);
                    let name_upper = name.as_deref().unwrap_or("").to_ascii_uppercase();
                    let parent_upper = parent_name.to_ascii_uppercase();

                    let resolved_type = raw_type.or_else(|| {
                        if name_upper.starts_with("CHK") || parent_upper.contains("CHECK") {
                            Some("Check Box".to_string())
                        } else if name_upper.starts_with("RAD") || parent_upper.contains("RADIO") {
                            Some("Radio Group".to_string())
                        } else if name_upper.starts_with("BTN")
                            || name_upper.contains("BUTTON")
                            || parent_upper.contains("BUTTON")
                            || (parent_upper.is_empty() && label.is_some() && prompt.is_none())
                        {
                            Some("Push Button".to_string())
                        } else {
                            None
                        }
                    });

                    let is_push_button = resolved_type
                        .as_deref()
                        .map(|t| t.eq_ignore_ascii_case("Push Button"))
                        .unwrap_or(false);
                    let items_disp = if is_push_button {
                        match raw_items_disp {
                            Some(n) if n > 1 => n,
                            _ => 1,
                        }
                    } else {
                        match raw_items_disp {
                            Some(n) if n > 0 => n,
                            _ => block_records,
                        }
                    };

                    let raw_canvas = get_attr(e, "CanvasName").filter(|s| !s.trim().is_empty());

                    let raw_prompt_edge = get_attr(e, "PromptAttachmentEdge");
                    let raw_prompt_offset = get_int_attr(e, "PromptAttachmentOffset");
                    let raw_prompt_align = get_attr(e, "PromptAlign");

                    *current_item = Some(ItemBuilder {
                        name: name.clone().unwrap_or_default(),
                        item_type: resolved_type,
                        data_type: get_attr(e, "DataType"),
                        column_name: get_attr(e, "ColumnName"),
                        canvas_name: raw_canvas,
                        prompt,
                        property_class: local_parent_name(e, module_name),
                        visible: Some(vis),
                        required: get_bool_attr(e, "Required"),
                        lov_name: get_attr(e, "LOVName").or_else(|| get_attr(e, "LovName")),
                        inherited,
                        dml: item_dml(e),
                        width: get_int_attr(e, "Width"),
                        height: get_int_attr(e, "Height"),
                        x_position: get_int_attr(e, "XPosition").unwrap_or(0),
                        y_position: get_int_attr(e, "YPosition").unwrap_or(0),
                        tab_page_name: get_attr(e, "TabPageName").filter(|s| !s.trim().is_empty()),
                        label,
                        prompt_edge_set: raw_prompt_edge.is_some(),
                        prompt_offset_set: raw_prompt_offset.is_some(),
                        prompt_align_set: raw_prompt_align.is_some(),
                        prompt_attachment_edge: raw_prompt_edge.unwrap_or_else(|| "Start".to_string()),
                        prompt_attachment_offset: raw_prompt_offset.unwrap_or(5),
                        prompt_align: raw_prompt_align.unwrap_or_else(|| "Start".to_string()),
                        prompt_align_offset: get_int_attr(e, "PromptAlignOffset").unwrap_or(0),
                        items_display: items_disp,
                        distance_between_records: dist,
                        back_color: get_attr(e, "BackColor"),
                        fore_color: get_attr(e, "ForegroundColor").or_else(|| get_attr(e, "ForeColor")),
                        fill_pattern: get_attr(e, "FillPattern"),
                        bevel: get_attr(e, "Bevel"),
                        font_name: get_attr(e, "FontName"),
                        font_size: get_int_attr(e, "FontSize"),
                        font_weight: get_attr(e, "FontWeight"),
                        font_style: get_attr(e, "FontStyle"),
                        visual_attribute: get_attr(e, "VisualAttributeName"),
                        prompt_fore_color: get_attr(e, "PromptForegroundColor"),
                        prompt_font_name: get_attr(e, "PromptFontName"),
                        prompt_font_size: get_int_attr(e, "PromptFontSize"),
                        prompt_font_weight: get_attr(e, "PromptFontWeight"),
                        radio_buttons: Vec::new(),
                        enabled,
                        canvas_visible: vis,
                        trigger_names: Vec::new(),
                        hint: get_attr(e, "Hint").map(|s| decode_double_escaped(&s).text),
                        access_key: get_attr(e, "AccessKey"),
                        auto_hint: get_bool_attr(e, "AutoHint"),
                        previous_navigation_item_name: get_attr(e, "PreviousNavigationItemName"),
                        keyboard_navigable: get_bool_attr(e, "KeyboardNavigable"),
                        mouse_navigate: get_bool_attr(e, "MouseNavigate"),
                        extra_attrs: collect_attrs(e),
                        extra_children: Vec::new(),
                    });
                }
            }
            "RadioButton" => {
                if parent_element.as_deref() == Some("Item") {
                    if let Some(it) = current_item.as_mut() {
                        it.radio_buttons.push(RadioButtonInfo {
                            name: name.clone().unwrap_or_default(),
                            label: get_attr(e, "Label").map(|s| decode_double_escaped(&s).text),
                            value: get_attr(e, "RadioButtonValue"),
                            x_position: get_int_attr(e, "XPosition").unwrap_or(0),
                            y_position: get_int_attr(e, "YPosition").unwrap_or(0),
                            width: get_int_attr(e, "Width"),
                            height: get_int_attr(e, "Height"),
                            visible: get_bool_attr(e, "Visible").unwrap_or(true),
                            back_color: get_attr(e, "BackColor"),
                            fore_color: get_attr(e, "ForegroundColor").or_else(|| get_attr(e, "ForeColor")),
                            font_name: get_attr(e, "FontName"),
                            font_size: get_int_attr(e, "FontSize"),
                        });
                    }
                }
            }
            "Canvas" => {
                *current_canvas = Some(CanvasBuilder {
                    name: name.clone().unwrap_or_default(),
                    canvas_type: get_attr(e, "CanvasType"),
                    window_name: get_attr(e, "WindowName"),
                    property_class: local_parent_name(e, module_name),
                    raise_on_enter: get_bool_attr(e, "RaiseOnEnter"),
                    width: get_int_attr(e, "Width").unwrap_or(800),
                    height: get_int_attr(e, "Height").unwrap_or(600),
                    viewport_width: get_int_attr(e, "ViewportWidth"),
                    viewport_height: get_int_attr(e, "ViewportHeight"),
                    viewport_x_position: get_int_attr(e, "ViewportXPosition").unwrap_or(0),
                    viewport_y_position: get_int_attr(e, "ViewportYPosition").unwrap_or(0),
                    viewport_x_position_on_canvas: get_int_attr(e, "ViewportXPositionOnCanvas"),
                    viewport_y_position_on_canvas: get_int_attr(e, "ViewportYPositionOnCanvas"),
                    back_color: get_attr(e, "BackColor"),
                    visual_attribute: get_attr(e, "VisualAttributeName"),
                    inherited,
                    tab_pages: Vec::new(),
                });
            }
            "TabPage" => {
                let page_name = name.clone().unwrap_or_default();
                let page_label = get_attr(e, "Label").map(|s| decode_double_escaped(&s).text);
                *current_tab_page_name = Some(page_name.clone());
                if let Some(c) = current_canvas.as_mut() {
                    c.tab_pages.push(TabPageInfo {
                        name: page_name,
                        label: page_label,
                        back_color: get_attr(e, "BackColor"),
                        fore_color: get_attr(e, "ForegroundColor").or_else(|| get_attr(e, "ForeColor")),
                        associated_canvases: Vec::new(),
                    });
                }
            }
            "Graphics" => {
                let g_type = get_attr(e, "GraphicsType").unwrap_or_else(|| "Rectangle".to_string());
                let g_text = get_attr(e, "GraphicsText")
                    .map(|s| decode_double_escaped(&s).text)
                    .or_else(|| get_attr(e, "Text").map(|s| decode_double_escaped(&s).text));
                let f_title = get_attr(e, "FrameTitle")
                    .or_else(|| get_attr(e, "Title"))
                    .map(|s| decode_double_escaped(&s).text);

                graphics.push(GraphicInfo {
                    name: name.clone().unwrap_or_else(|| format!("GRAPHIC_{}", graphics.len() + 1)),
                    canvas_name: current_canvas.as_ref().map(|c| c.name.clone()).or_else(|| get_attr(e, "CanvasName")),
                    tab_page_name: current_tab_page_name.clone().or_else(|| get_attr(e, "TabPageName")),
                    graphics_type: g_type,
                    graphics_text: g_text,
                    frame_title: f_title,
                    x_position: get_int_attr(e, "XPosition").unwrap_or(0),
                    y_position: get_int_attr(e, "YPosition").unwrap_or(0),
                    width: get_int_attr(e, "Width").unwrap_or(0),
                    height: get_int_attr(e, "Height").unwrap_or(0),
                    bevel: get_attr(e, "Bevel"),
                    back_color: get_attr(e, "BackColor"),
                    fore_color: get_attr(e, "ForegroundColor")
                        .or_else(|| get_attr(e, "ForeColor"))
                        .or_else(|| get_attr(e, "GraphicsFontColor")),
                    edge_foreground_color: get_attr(e, "EdgeForegroundColor"),
                    edge_back_color: get_attr(e, "EdgeBackColor"),
                    fill_pattern: get_attr(e, "FillPattern"),
                    line_width: get_int_attr(e, "InternalLineWidth").or_else(|| get_int_attr(e, "LineWidth")),
                    font_name: get_attr(e, "GraphicsFontName").or_else(|| get_attr(e, "FontName")),
                    font_size: get_int_attr(e, "GraphicsFontSize").or_else(|| get_int_attr(e, "FontSize")),
                    font_weight: get_attr(e, "GraphicsFontWeight").or_else(|| get_attr(e, "FontWeight")),
                    visual_attribute: get_attr(e, "VisualAttributeName"),
                    frame_title_fore_color: get_attr(e, "FrameTitleForegroundColor"),
                    frame_title_back_color: get_attr(e, "FrameTitleBackColor"),
                    frame_title_font_name: get_attr(e, "FrameTitleFontName"),
                    frame_title_font_size: get_int_attr(e, "FrameTitleFontSize"),
                    frame_title_font_weight: get_attr(e, "FrameTitleFontWeight"),
                    frame_title_visual_attribute: get_attr(e, "FrameTitleVisualAttributeName"),
                    inherited,
                    points: Vec::new(),
                    compound_texts: Vec::new(),
                    children: Vec::new(),
                    extra_attrs: collect_attrs(e),
                });
            }
            "CompoundText" => {
                if parent_element.as_deref() == Some("Graphics") {
                    if let Some(g) = graphics.last_mut() {
                        g.compound_texts.push(CompoundTextInfo {
                            name: name.clone().unwrap_or_else(|| format!("CMPTXT_{}", g.compound_texts.len() + 1)),
                            text_segments: Vec::new(),
                            extra_attrs: collect_attrs(e),
                        });
                    }
                }
            }
            "TextSegment" => {
                if parent_element.as_deref() == Some("CompoundText") {
                    if let Some(g) = graphics.last_mut() {
                        let seg_text = get_attr(e, "Text").map(|s| decode_double_escaped(&s).text);
                        if let Some(ref st) = seg_text {
                            match g.graphics_text {
                                Some(ref mut existing) if !existing.is_empty() => {
                                    existing.push('\n');
                                    existing.push_str(st);
                                }
                                _ => {
                                    g.graphics_text = Some(st.clone());
                                }
                            }
                        }
                        if let Some(ct) = g.compound_texts.last_mut() {
                            ct.text_segments.push(TextSegmentInfo {
                                name: name.clone().unwrap_or_else(|| format!("TEXTSTR_{}", ct.text_segments.len() + 1)),
                                text: seg_text,
                                font_name: get_attr(e, "FontName"),
                                font_size: get_int_attr(e, "FontSize"),
                                font_weight: get_attr(e, "FontWeight"),
                                font_style: get_attr(e, "FontStyle"),
                                font_spacing: get_attr(e, "FontSpacing"),
                                foreground_color: get_attr(e, "ForegroundColor"),
                                extra_attrs: collect_attrs(e),
                            });
                        }
                    }
                }
            }
            "ObjectGroup" => {
                if parent_element.as_deref() == Some("FormModule") {
                    object_groups.push(ObjectGroupInfo {
                        name: name.clone().unwrap_or_default(),
                        children: Vec::new(),
                        extra_attrs: collect_attrs(e),
                    });
                }
            }
            "ObjectGroupChild" => {
                if parent_element.as_deref() == Some("ObjectGroup") {
                    if let Some(og) = object_groups.last_mut() {
                        og.children.push(ObjectGroupChildInfo {
                            name: name.clone().unwrap_or_default(),
                            subclass_sub_object: get_bool_attr(e, "SubclassSubObject"),
                            object_type: get_attr(e, "Type"),
                            program_unit_type: get_attr(e, "ProgramUnitType"),
                            extra_attrs: collect_attrs(e),
                        });
                    }
                }
            }
            "Report" => {
                if parent_element.as_deref() == Some("FormModule") {
                    form_reports.push(FormReportInfo {
                        name: name.clone().unwrap_or_default(),
                        filename: get_attr(e, "Filename"),
                        execution_mode: get_attr(e, "ExecutionMode"),
                        comm_mode: get_attr(e, "CommMode"),
                        destype: get_attr(e, "Destype"),
                        desformat: get_attr(e, "Desformat"),
                        desname: get_attr(e, "Desname"),
                        extra_attrs: collect_attrs(e),
                    });
                }
            }
            "Trigger" => {
                let raw_text = get_attr(e, "TriggerText").unwrap_or_default();
                let body = decode_double_escaped(&raw_text);
                let text = body.text;
                let level = match parent_element.as_deref() {
                    Some("Block") => TriggerLevel::Block,
                    Some("Item") => TriggerLevel::Item,
                    Some("Menu") | Some("MenuItem") => TriggerLevel::Menu,
                    Some("PropertyClass") => TriggerLevel::PropertyClass,
                    _ => TriggerLevel::Form,
                };
                let trigger_name = name.clone().unwrap_or_default();
                let scope = owner_path.clone().unwrap_or_else(|| "FORM".to_string());
                let text_ref = sidecars.write(
                    PlsqlSidecars::TRIGGERS,
                    &format!("{}.{}", scope, trigger_name),
                    &text,
                )?;

                let block_name = match level {
                    TriggerLevel::Block | TriggerLevel::Item => current_block.as_ref().map(|b| b.name.clone()),
                    _ => None,
                };
                let item_name = match level {
                    TriggerLevel::Item => current_item.as_ref().map(|i| i.name.clone()),
                    _ => None,
                };

                match level {
                    TriggerLevel::Block => {
                        if let Some(b) = current_block.as_mut() {
                            b.trigger_names.push(trigger_name.clone());
                        }
                    }
                    TriggerLevel::Item => {
                        if let Some(i) = current_item.as_mut() {
                            i.trigger_names.push(trigger_name.clone());
                        }
                    }
                    _ => {}
                }

                let trig = TriggerInfo {
                    name: trigger_name,
                    level,
                    block_name,
                    item_name,
                    first_line: first_code_line(&text),
                    line_count: line_count_of(&text),
                    inherited,
                    text_encoding: body.encoding,
                    text_ref: Some(text_ref),
                    xml_ref: None,
                    source_text: Some(text.clone()),
                    execution_hierarchy: get_attr(e, "ExecuteHierarchy").or_else(|| get_attr(e, "ExecutionHierarchy")),
                };
                if level == TriggerLevel::PropertyClass {
                    if let Some(pc) = property_class_details.last_mut() {
                        pc.triggers.push(trig);
                    }
                } else {
                    triggers.push(trig);
                }
            }
            "ProgramUnit" => {
                let unit_type_str = get_attr(e, "ProgramUnitType").unwrap_or_default();
                let unit_type = ProgramUnitType::from_forms(&unit_type_str);
                let raw_text = get_attr(e, "ProgramUnitText").unwrap_or_default();
                let body = decode_double_escaped(&raw_text);
                let text = body.text;
                let unit_name = name.clone().unwrap_or_default();
                let type_name = format!("{:?}", unit_type).to_ascii_uppercase();
                let text_ref = sidecars.write(
                    PlsqlSidecars::PROGRAM_UNITS,
                    &format!("{}.{}", unit_name, type_name),
                    &text,
                )?;

                program_units.push(ProgramUnitInfo {
                    name: unit_name,
                    unit_type,
                    line_count: line_count_of(&text),
                    first_line: first_code_line(&text),
                    inherited,
                    text_encoding: body.encoding,
                    text_ref: Some(text_ref),
                    xml_ref: None,
                    source_text: Some(text),
                });
            }
            "AttachedLibrary" => {
                attached_libraries.push(AttachedLibraryInfo {
                    name: name.clone().unwrap_or_default(),
                    library_location: get_attr(e, "LibraryLocation"),
                });
            }
            "LOV" => {
                *current_lov = Some(LovInfo {
                    name: name.clone().unwrap_or_default(),
                    record_group_name: get_attr(e, "RecordGroupName"),
                    column_mappings: Vec::new(),
                });
            }
            "LOVColumnMapping" => {
                if let Some(l) = current_lov.as_mut() {
                    if let Some(ref n) = name {
                        l.column_mappings.push(n.clone());
                    }
                }
            }
            "RecordGroup" => {
                *current_record_group = Some(RecordGroupInfo {
                    name: name.clone().unwrap_or_default(),
                    query_text: get_attr(e, "RecordGroupQuery"),
                    columns: Vec::new(),
                });
            }
            "RecordGroupColumn" => {
                if let Some(rg) = current_record_group.as_mut() {
                    if let Some(ref n) = name {
                        rg.columns.push(n.clone());
                    }
                }
            }
            "Window" => {
                windows.push(WindowInfo {
                    name: name.clone().unwrap_or_default(),
                    title: get_attr(e, "Title"),
                    modal: get_bool_attr(e, "Modal"),
                    width: get_int_attr(e, "Width"),
                    height: get_int_attr(e, "Height"),
                    horizontal_toolbar_canvas_name: get_attr(e, "HorizontalToolbarCanvasName"),
                    vertical_toolbar_canvas_name: get_attr(e, "VerticalToolbarCanvasName"),
                });
            }
            "Alert" => {
                alerts.push(AlertInfo {
                    name: name.clone().unwrap_or_default(),
                    message: get_attr(e, "AlertMessage").or_else(|| get_attr(e, "Message")),
                    title: get_attr(e, "Title"),
                    button1_label: get_attr(e, "Button1Label"),
                    button2_label: get_attr(e, "Button2Label"),
                    button3_label: get_attr(e, "Button3Label"),
                    default_button: get_attr(e, "DefaultAlertButton").and_then(|b| match b.trim() {
                        "Button 1" | "0" => Some(1),
                        "Button 2" | "1" => Some(2),
                        "Button 3" | "2" => Some(3),
                        _ => None,
                    }),
                });
            }
            "ModuleParameter" => {
                parameters.push(ParameterInfo {
                    name: name.clone().unwrap_or_default(),
                    data_type: get_attr(e, "ParameterDataType").or_else(|| get_attr(e, "DataType")),
                    default_value: get_attr(e, "ParameterInitializeValue").or_else(|| get_attr(e, "DefaultValue")),
                });
            }
            "VisualAttribute" => {
                if let Some(ref n) = name {
                    if current_tab.is_none() {
                        visual_attributes.push(n.clone());
                    }
                    visual_attribute_details.push(VisualAttributeInfo {
                        name: n.clone(),
                        back_color: get_attr(e, "BackColor"),
                        fore_color: get_attr(e, "ForegroundColor").or_else(|| get_attr(e, "ForeColor")),
                        fill_pattern: get_attr(e, "FillPattern"),
                        font_name: get_attr(e, "FontName"),
                        font_size: get_int_attr(e, "FontSize"),
                        font_weight: get_attr(e, "FontWeight"),
                        font_style: get_attr(e, "FontStyle"),
                        prompt_fore_color: get_attr(e, "PromptForegroundColor"),
                        prompt_font_name: get_attr(e, "PromptFontName"),
                        prompt_font_size: get_int_attr(e, "PromptFontSize"),
                        prompt_font_weight: get_attr(e, "PromptFontWeight"),
                        inherited,
                    });
                }
            }
            "PropertyClass" => {
                if let Some(ref n) = name {
                    property_classes.push(n.clone());
                    property_class_details.push(PropertyClassInfo {
                        name: n.clone(),
                        item: item_dml(e),
                        block: block_dml(e, false),
                        geometry: item_geometry(e),
                        property_class: local_parent_name(e, module_name),
                        inherited,
                        style: visual_style(e),
                        item_type: get_attr(e, "ItemType"),
                        prompt: get_attr(e, "Prompt").map(|s| decode_double_escaped(&s).text),
                        label: get_attr(e, "Label").map(|s| decode_double_escaped(&s).text),
                        prompt_attachment_edge: get_attr(e, "PromptAttachmentEdge"),
                        prompt_attachment_offset: get_int_attr(e, "PromptAttachmentOffset"),
                        prompt_align: get_attr(e, "PromptAlign"),
                        items_display: get_int_attr(e, "ItemsDisplay").or_else(|| get_int_attr(e, "ItemsDisplayed")),
                        distance_between_records: get_int_attr(e, "DistanceBetweenRecords"),
                        triggers: Vec::new(),
                        smart_class: get_bool_attr(e, "SmartClass"),
                        extra_attrs: collect_attrs(e),
                    });
                }
            }
            "Editor" => {
                if let Some(ref n) = name {
                    editors.push(n.clone());
                }
            }
            "Menu" => {
                *current_menu = Some(MenuInfo {
                    name: name.clone().unwrap_or_default(),
                    items: Vec::new(),
                });
            }
            "MenuItem" => {
                if let Some(m) = current_menu.as_mut() {
                    let item_name = name.clone().unwrap_or_default();
                    let command = get_attr(e, "CommandText")
                        .or_else(|| get_attr(e, "MenuItemCode"))
                        .map(|s| decode_double_escaped(&s));
                    let command_ref = if let Some(ref cmd) = command {
                        Some(sidecars.write(
                            PlsqlSidecars::MENU_ITEMS,
                            &format!("{}.{}", m.name, item_name),
                            &cmd.text,
                        )?)
                    } else {
                        None
                    };
                    m.items.push(MenuItemInfo {
                        name: item_name,
                        label: get_attr(e, "Label"),
                        command_type: get_attr(e, "CommandType"),
                        sub_menu_name: get_attr(e, "SubMenuName"),
                        menu_item_type: get_attr(e, "MenuItemType"),
                        command_ref,
                        command_text: command.as_ref().map(|c| c.text.clone()),
                        text_encoding: command.map(|c| c.encoding).unwrap_or_default(),
                    });
                }
            }
            "ObjectLibraryTab" => {
                *current_tab = Some(ObjectLibraryTabInfo {
                    name: name.clone().unwrap_or_default(),
                    entries: Vec::new(),
                });
            }
            _ => {
                if element != "Module" && parent_element.as_deref() != Some("ObjectLibraryTab") {
                    raw_node_stack.push((
                        stack.len(),
                        RawXmlNode {
                            tag: element.clone(),
                            attrs: collect_attrs(e),
                            children: Vec::new(),
                        },
                    ));
                }
            }
        }

        if let Some(t) = current_tab.as_mut() {
            if parent_element.as_deref() == Some("ObjectLibraryTab") {
                if let Some(ref n) = name {
                    t.entries.push(ObjectLibraryEntry {
                        name: n.clone(),
                        object_type: Some(element),
                    });
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_end_element(
        end_line: usize,
        xml_path: &str,
        stack: &mut Vec<Frame>,
        object_refs: &mut Vec<ObjectRef>,
        triggers: &mut [TriggerInfo],
        program_units: &mut [ProgramUnitInfo],
        current_block: &mut Option<BlockBuilder>,
        current_item: &mut Option<ItemBuilder>,
        blocks: &mut Vec<BlockInfo>,
        current_canvas: &mut Option<CanvasBuilder>,
        canvases: &mut Vec<CanvasInfo>,
        current_tab_page_name: &mut Option<String>,
        current_lov: &mut Option<LovInfo>,
        lovs: &mut Vec<LovInfo>,
        current_record_group: &mut Option<RecordGroupInfo>,
        record_groups: &mut Vec<RecordGroupInfo>,
        current_menu: &mut Option<MenuInfo>,
        menus: &mut Vec<MenuInfo>,
        current_tab: &mut Option<ObjectLibraryTabInfo>,
        object_library_tabs: &mut Vec<ObjectLibraryTabInfo>,
        module_extra_children: &mut Vec<RawXmlNode>,
        raw_node_stack: &mut Vec<(usize, RawXmlNode)>,
    ) {
        let depth = stack.len();
        let frame = match stack.pop() {
            Some(f) => f,
            None => return,
        };
        if raw_node_stack.last().map(|(d, _)| *d == depth).unwrap_or(false) {
            if let Some((_, node)) = raw_node_stack.pop() {
                if let Some((_, parent_raw)) = raw_node_stack.last_mut() {
                    parent_raw.children.push(node);
                } else {
                    match stack.last().map(|p| p.element.as_str()) {
                        Some("Block") => {
                            if let Some(b) = current_block.as_mut() {
                                b.extra_children.push(node);
                            }
                        }
                        Some("Item") => {
                            if let Some(it) = current_item.as_mut() {
                                it.extra_children.push(node);
                            }
                        }
                        Some("FormModule" | "MenuModule" | "ObjectLibrary") => {
                            module_extra_children.push(node);
                        }
                        _ => {}
                    }
                }
            }
            return;
        }
        let final_end_line = end_line.max(frame.start_line);

        if let Some(ref name) = frame.name {
            object_refs.push(ObjectRef {
                object_type: frame.element.clone(),
                name: name.clone(),
                owner_path: frame.owner_path.clone(),
                inherited: frame.inherited.clone(),
                reference: SourceRef::new(xml_path, frame.start_line, final_end_line),
            });
        }

        match frame.element.as_str() {
            "Trigger" => {
                if let Some(last) = triggers.last_mut() {
                    if last.xml_ref.is_none() {
                        last.xml_ref = Some(SourceRef::new(xml_path, frame.start_line, final_end_line));
                    }
                }
            }
            "ProgramUnit" => {
                if let Some(last) = program_units.last_mut() {
                    if last.xml_ref.is_none() {
                        last.xml_ref = Some(SourceRef::new(xml_path, frame.start_line, final_end_line));
                    }
                }
            }
            "Block" => {
                if let Some(b) = current_block.take() {
                    blocks.push(BlockInfo {
                        name: b.name,
                        query_data_source_name: b.query_data_source_name,
                        property_class: b.property_class,
                        items: b.items,
                        trigger_names: b.trigger_names,
                        inherited: b.inherited,
                        source_ref: Some(SourceRef::new(xml_path, frame.start_line, final_end_line)),
                        dml: b.dml,
                        data_source_column_count: b.data_source_column_count,
                        relations: b.relations,
                        records_displayed: b.records_displayed,
                        distance_between_records: b.distance_between_records,
                        record_orientation: b.record_orientation,
                        navigation_style: b.navigation_style,
                        next_navigation_block_name: b.next_navigation_block_name,
                        previous_navigation_block_name: b.previous_navigation_block_name,
                        show_scrollbar: b.show_scrollbar,
                        visual_attribute: b.visual_attribute,
                        dml_data_type: b.dml_data_type,
                        query_data_source_type: b.query_data_source_type,
                        extra_attrs: b.extra_attrs,
                        extra_children: b.extra_children,
                    });
                }
            }
            "Item" => {
                if let Some(i) = current_item.take() {
                    if let Some(b) = current_block.as_mut() {
                        b.items.push(ItemInfo {
                            name: i.name,
                            item_type: i.item_type,
                            data_type: i.data_type,
                            column_name: i.column_name,
                            canvas_name: i.canvas_name,
                            prompt: i.prompt,
                            property_class: i.property_class,
                            visible: i.visible,
                            required: i.required,
                            lov_name: i.lov_name,
                            trigger_names: i.trigger_names,
                            inherited: i.inherited,
                            dml: i.dml,
                            width: i.width,
                            height: i.height,
                            x_position: i.x_position,
                            y_position: i.y_position,
                            tab_page_name: i.tab_page_name,
                            label: i.label,
                            prompt_attachment_edge: i.prompt_attachment_edge,
                            prompt_attachment_offset: i.prompt_attachment_offset,
                            prompt_align: i.prompt_align,
                            prompt_align_offset: i.prompt_align_offset,
                            prompt_edge_set: i.prompt_edge_set,
                            prompt_offset_set: i.prompt_offset_set,
                            prompt_align_set: i.prompt_align_set,
                            items_display: i.items_display,
                            distance_between_records: i.distance_between_records,
                            back_color: i.back_color,
                            fore_color: i.fore_color,
                            fill_pattern: i.fill_pattern,
                            bevel: i.bevel,
                            font_name: i.font_name,
                            font_size: i.font_size,
                            font_weight: i.font_weight,
                            font_style: i.font_style,
                            visual_attribute: i.visual_attribute,
                            prompt_fore_color: i.prompt_fore_color,
                            prompt_font_name: i.prompt_font_name,
                            prompt_font_size: i.prompt_font_size,
                            prompt_font_weight: i.prompt_font_weight,
                            radio_buttons: i.radio_buttons,
                            list_elements: Vec::new(),
                            enabled: i.enabled,
                            canvas_visible: i.canvas_visible,
                            hint: i.hint,
                            access_key: i.access_key,
                            auto_hint: i.auto_hint,
                            previous_navigation_item_name: i.previous_navigation_item_name,
                            keyboard_navigable: i.keyboard_navigable,
                            mouse_navigate: i.mouse_navigate,
                            extra_attrs: i.extra_attrs,
                            extra_children: i.extra_children,
                        });
                    }
                }
            }
            "Canvas" => {
                if let Some(c) = current_canvas.take() {
                    canvases.push(CanvasInfo {
                        name: c.name,
                        canvas_type: c.canvas_type,
                        window_name: c.window_name,
                        property_class: c.property_class,
                        raise_on_enter: c.raise_on_enter,
                        width: c.width,
                        height: c.height,
                        viewport_width: c.viewport_width,
                        viewport_height: c.viewport_height,
                        viewport_x_position: c.viewport_x_position,
                        viewport_y_position: c.viewport_y_position,
                        viewport_x_position_on_canvas: c.viewport_x_position_on_canvas,
                        viewport_y_position_on_canvas: c.viewport_y_position_on_canvas,
                        back_color: c.back_color,
                        visual_attribute: c.visual_attribute,
                        inherited: c.inherited,
                        tab_pages: c.tab_pages,
                    });
                }
            }
            "TabPage" => {
                *current_tab_page_name = None;
            }
            "LOV" => {
                if let Some(l) = current_lov.take() {
                    lovs.push(l);
                }
            }
            "RecordGroup" => {
                if let Some(rg) = current_record_group.take() {
                    record_groups.push(rg);
                }
            }
            "Menu" => {
                if let Some(m) = current_menu.take() {
                    menus.push(m);
                }
            }
            "ObjectLibraryTab" => {
                if let Some(t) = current_tab.take() {
                    object_library_tabs.push(t);
                }
            }
            _ => {}
        }
    }
}

fn collect_attrs(e: &BytesStart) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for a in e.attributes().flatten() {
        if let (Ok(k), Ok(v)) = (
            String::from_utf8(a.key.as_ref().to_vec()),
            String::from_utf8(a.value.to_vec()),
        ) {
            map.insert(k, unescape_xml_entities(&v));
        }
    }
    map
}

fn get_attr(e: &BytesStart, name: &str) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == name.as_bytes() {
            return String::from_utf8(a.value.to_vec())
                .ok()
                .map(|raw| unescape_xml_entities(&raw));
        }
    }
    None
}

fn get_bool_attr(e: &BytesStart, name: &str) -> Option<bool> {
    let val = get_attr(e, name)?;
    match val.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn get_int_attr(e: &BytesStart, name: &str) -> Option<i32> {
    get_attr(e, name)?.trim().parse::<i32>().ok()
}

fn item_dml(e: &BytesStart) -> Option<ItemDml> {
    let dml = ItemDml {
        database_item: get_bool_attr(e, "DatabaseItem"),
        insert_allowed: get_bool_attr(e, "InsertAllowed"),
        update_allowed: get_bool_attr(e, "UpdateAllowed"),
        update_if_null: get_bool_attr(e, "UpdateIfNull"),
        query_allowed: get_bool_attr(e, "QueryAllowed"),
        enabled: get_bool_attr(e, "Enabled"),
        keyboard_navigable: get_bool_attr(e, "KeyboardNavigable"),
        primary_key: get_bool_attr(e, "PrimaryKey"),
        required: get_bool_attr(e, "Required"),
        maximum_length: get_int_attr(e, "MaximumLength"),
        initial_value: get_attr(e, "InitializeValue"),
        copy_value_from_item: get_attr(e, "CopyValueFromItem"),
    };
    if dml.is_empty() { None } else { Some(dml) }
}

fn visual_style(e: &BytesStart) -> Option<VisualStyle> {
    let style = VisualStyle {
        back_color: get_attr(e, "BackColor"),
        fore_color: get_attr(e, "ForegroundColor").or_else(|| get_attr(e, "ForeColor")),
        fill_pattern: get_attr(e, "FillPattern"),
        font_name: get_attr(e, "FontName"),
        font_size: get_int_attr(e, "FontSize"),
        font_weight: get_attr(e, "FontWeight"),
        font_style: get_attr(e, "FontStyle"),
        bevel: get_attr(e, "Bevel"),
        visual_attribute: get_attr(e, "VisualAttributeName"),
        prompt_fore_color: get_attr(e, "PromptForegroundColor"),
        prompt_font_name: get_attr(e, "PromptFontName"),
        prompt_font_size: get_int_attr(e, "PromptFontSize"),
        prompt_font_weight: get_attr(e, "PromptFontWeight"),
    };
    if style.is_empty() { None } else { Some(style) }
}

fn item_geometry(e: &BytesStart) -> Option<ItemGeometry> {
    let geom = ItemGeometry {
        width: get_int_attr(e, "Width"),
        height: get_int_attr(e, "Height"),
    };
    if geom.is_empty() { None } else { Some(geom) }
}

fn block_dml(e: &BytesStart, also_recovered: bool) -> Option<BlockDml> {
    let where_clause = get_attr(e, "WhereClause").map(|s| decode_double_escaped(&s));
    let order_by_clause = get_attr(e, "OrderByClause").map(|s| decode_double_escaped(&s));
    let recovered = also_recovered
        || where_clause.as_ref().map(|w| w.encoding == TextEncoding::Recovered).unwrap_or(false)
        || order_by_clause.as_ref().map(|o| o.encoding == TextEncoding::Recovered).unwrap_or(false);

    let dml = BlockDml {
        database_block: get_bool_attr(e, "DatabaseBlock"),
        insert_allowed: get_bool_attr(e, "InsertAllowed"),
        update_allowed: get_bool_attr(e, "UpdateAllowed"),
        delete_allowed: get_bool_attr(e, "DeleteAllowed"),
        query_allowed: get_bool_attr(e, "QueryAllowed"),
        key_mode: get_attr(e, "KeyMode"),
        lock_mode: get_attr(e, "LockMode"),
        dml_data_target_name: get_attr(e, "DMLDataTargetName"),
        where_clause: where_clause.map(|w| w.text),
        order_by_clause: order_by_clause.map(|o| o.text),
        sql_encoding: if recovered { TextEncoding::Recovered } else { TextEncoding::Original },
    };
    if dml.is_empty() { None } else { Some(dml) }
}

fn local_parent_name(e: &BytesStart, module_name: &str) -> Option<String> {
    if get_attr(e, "ParentFilename").is_some() {
        return None;
    }
    if get_attr(e, "SubclassObjectGroup").map(|s| s.eq_ignore_ascii_case("true")).unwrap_or(false) {
        return None;
    }
    if let Some(pm) = get_attr(e, "ParentModule") {
        if !pm.eq_ignore_ascii_case(module_name) {
            return None;
        }
    }
    get_attr(e, "ParentName")
}

fn inheritance_of(
    e: &BytesStart,
    name: Option<&str>,
    owner_ref: Option<&InheritanceRef>,
    module_name: &str,
) -> Option<InheritanceRef> {
    let parent_module = get_attr(e, "ParentModule");
    let parent_filename = get_attr(e, "ParentFilename");
    let parent_name = get_attr(e, "ParentName");
    let parent_type = get_attr(e, "ParentType");
    let object_group = get_attr(e, "SubclassObjectGroup").map(|s| s.eq_ignore_ascii_case("true")).unwrap_or(false);
    let sub_object = get_attr(e, "SubclassSubObject").map(|s| s.eq_ignore_ascii_case("true")).unwrap_or(false);

    let elsewhere = parent_filename.is_some()
        || (parent_module.is_some() && !parent_module.as_ref().unwrap().eq_ignore_ascii_case(module_name));

    if elsewhere && object_group {
        Some(InheritanceRef {
            module: parent_module,
            file: parent_filename,
            name: name.map(|s| s.to_string()),
            owner_path: owner_ref.and_then(|o| join_path(o.owner_path.as_deref(), o.name.as_deref())),
            object_group: parent_name,
            parent_type,
            sub_object: false,
        })
    } else if elsewhere {
        Some(InheritanceRef {
            module: parent_module,
            file: parent_filename,
            name: parent_name.or_else(|| name.map(|s| s.to_string())),
            owner_path: owner_ref.and_then(|o| join_path(o.owner_path.as_deref(), o.name.as_deref())),
            object_group: None,
            parent_type,
            sub_object: false,
        })
    } else if let (true, Some(o)) = (sub_object, owner_ref) {
        Some(InheritanceRef {
            module: o.module.clone(),
            file: o.file.clone(),
            name: name.map(|s| s.to_string()),
            owner_path: join_path(o.owner_path.as_deref(), o.name.as_deref()),
            object_group: o.object_group.clone(),
            parent_type,
            sub_object: true,
        })
    } else if sub_object {
        Some(InheritanceRef {
            parent_type,
            sub_object: true,
            ..Default::default()
        })
    } else {
        None
    }
}

fn join_path(owner_path: Option<&str>, name: Option<&str>) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(op) = owner_path {
        if !op.is_empty() {
            parts.push(op);
        }
    }
    if let Some(n) = name {
        if !n.is_empty() {
            parts.push(n);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

fn owner_path_of(stack: &[Frame]) -> Option<String> {
    let names: Vec<&str> = stack
        .iter()
        .filter(|f| !matches!(f.element.as_str(), "Module" | "FormModule" | "MenuModule" | "ObjectLibrary"))
        .filter_map(|f| f.name.as_deref())
        .collect();
    if names.is_empty() {
        None
    } else {
        Some(names.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_graphics_fill_pattern_multiline_prompts_items_display_and_radio_buttons() {
        let dir = std::env::temp_dir().join(format!("ofw-xml-parity-{}", std::process::id()));
        let cache = dir.join("cache");
        fs::create_dir_all(&cache).unwrap();
        let xml_path = dir.join("dept_fmb.xml");
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Module version="14.1.2.0.0" xmlns="http://xmlns.oracle.com/Forms">
  <FormModule Name="DEPT_MOD">
    <Block Name="DEPT" RecordsDisplayCount="5">
      <Item Name="DEPTNO" ItemType="Text Item" CanvasName="MAIN" Prompt="Dept&amp;#10;Number" ItemsDisplay="0"/>
      <Item Name="BTN_SAVE" ItemType="Push Button" CanvasName="MAIN" Label="Save&#10;Record"/>
      <Item Name="BTN_ROW" ItemType="Push Button" CanvasName="MAIN" Label="Edit" ItemsDisplay="5"/>
      <Item Name="RG_STATUS" ItemType="Radio Group" CanvasName="MAIN" XPosition="10" YPosition="100" Width="120" Height="50">
        <RadioButton Name="RB_ACTIVE" Label="Active" RadioButtonValue="A" XPosition="5" YPosition="8"/>
        <RadioButton Name="RB_INACTIVE" Label="Inactive" RadioButtonValue="I" XPosition="5" YPosition="26"/>
      </Item>
    </Block>
    <Canvas Name="MAIN" CanvasType="Content" Width="600" Height="400">
      <Graphics Name="HDR_BOX" GraphicsType="Rectangle" FillPattern="transparent" BackColor="black" EdgeForegroundColor="darkblue" InternalLineWidth="2" XPosition="10" YPosition="10" Width="200" Height="30"/>
    </Canvas>
    <Canvas Name="STK_PANEL" CanvasType="Stacked" Width="400" Height="300" ViewportWidth="250" ViewportHeight="150" ViewportXPositionOnCanvas="20" ViewportYPositionOnCanvas="40" ViewportXPosition="5" ViewportYPosition="10"/>
  </FormModule>
</Module>"#;
        fs::write(&xml_path, xml).unwrap();

        let key = ModuleKey::new("DEPT_MOD", ModuleType::Form);
        let idx = FormsXmlParser::parse(&key, &xml_path, &cache).unwrap();

        let dept = &idx.blocks[0];
        assert_eq!(dept.items[0].prompt.as_deref(), Some("Dept\nNumber"));
        assert_eq!(dept.items[0].items_display, 5);
        assert_eq!(dept.items[1].label.as_deref(), Some("Save\nRecord"));
        assert_eq!(dept.items[1].items_display, 1);
        assert_eq!(dept.items[2].items_display, 5);
        assert_eq!(dept.items[3].radio_buttons.len(), 2);
        assert_eq!(dept.items[3].radio_buttons[0].value.as_deref(), Some("A"));

        let g = &idx.graphics[0];
        assert_eq!(g.fill_pattern.as_deref(), Some("transparent"));
        assert_eq!(g.edge_foreground_color.as_deref(), Some("darkblue"));
        assert_eq!(g.line_width, Some(2));

        let stk = &idx.canvases[1];
        assert_eq!(stk.viewport_x_position_on_canvas, Some(20));
        assert_eq!(stk.viewport_y_position_on_canvas, Some(40));

        let _ = fs::remove_dir_all(&dir);
    }
}
