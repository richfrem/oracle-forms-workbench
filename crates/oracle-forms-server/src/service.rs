use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use oracle_forms_binary::BinaryFmbParser;
use oracle_forms_core::error::{FormsError, Result};
use oracle_forms_core::model::*;
use oracle_forms_xml::{FormsXmlParser, PldParser};
use crate::security::{is_plain_file_name, safe_join};

#[derive(Default)]
struct ServiceState {
    modules: HashMap<String, ModuleIndex>,
    converted_files: HashMap<String, PathBuf>,
    source_files: HashMap<String, PathBuf>,
    cache_dirs: HashMap<String, PathBuf>,
    /// Edits made in the layout editor, applied to the original XML on export.
    item_edits: HashMap<String, Vec<oracle_forms_xml::ItemPatch>>,
    model_edited: HashSet<String>,
}

/// How an export was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportMode {
    /// The original Forms2XML file with only the edited attributes changed (lossless).
    PatchedOriginal,
    /// Regenerated from the parsed model: properties that are not modelled are omitted.
    Summary,
}

#[derive(Clone)]
pub struct FormsService {
    base_cache_dir: PathBuf,
    state: Arc<RwLock<ServiceState>>,
    /// When true, requests that omit DB credentials may use `OFW_DB_USER`/`OFW_DB_PASSWORD`.
    /// Off by default: a request must never be able to borrow the operator's credentials
    /// unless the operator opted in when starting the server.
    allow_env_credentials: bool,
    /// Live form sessions (triggers and PL/SQL running on the server).
    pub sessions: crate::runtime_sessions::SessionManager,
}

impl FormsService {
    pub fn new(base_cache_dir: impl Into<PathBuf>) -> Self {
        let dir = base_cache_dir.into();
        let _ = fs::create_dir_all(&dir);
        Self {
            base_cache_dir: dir,
            state: Arc::new(RwLock::new(ServiceState::default())),
            allow_env_credentials: false,
            sessions: Default::default(),
        }
    }

    /// Opt in to filling missing request credentials from the server's environment.
    pub fn with_env_credentials(mut self, allow: bool) -> Self {
        self.allow_env_credentials = allow;
        self
    }

    /// Applies the server's credential policy to a request-built config.
    pub fn apply_credential_policy(&self, config: oracle_forms_db::DbConfig) -> oracle_forms_db::DbConfig {
        if self.allow_env_credentials {
            config.with_env_credentials()
        } else {
            config
        }
    }

    pub fn samples_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("OFW_SAMPLES_DIR") {
            return PathBuf::from(dir);
        }
        if Path::new("samples").is_dir() {
            return PathBuf::from("samples");
        }
        if Path::new("../../samples").is_dir() {
            return PathBuf::from("../../samples");
        }
        if Path::new("../samples").is_dir() {
            return PathBuf::from("../samples");
        }
        PathBuf::from("samples")
    }

    /// Extra host/WSL directories from `OFW_FORMS_PATH_EXTRA` (env or `.env`, colon-separated).
    /// Read-only source locations for loading external forms without copying them into `samples/`.
    pub fn extra_forms_dirs() -> Vec<PathBuf> {
        let raw = std::env::var("OFW_FORMS_PATH_EXTRA").ok().filter(|v| !v.trim().is_empty()).or_else(|| {
            let env_path = Self::samples_dir().parent().map(|p| p.join(".env")).unwrap_or_else(|| PathBuf::from(".env"));
            fs::read_to_string(env_path).ok().and_then(|text| {
                text.lines().find_map(|line| {
                    let val = line.trim().strip_prefix("OFW_FORMS_PATH_EXTRA=")?.trim().trim_matches('"').trim_matches('\'');
                    (!val.is_empty()).then(|| val.to_string())
                })
            })
        });
        raw.unwrap_or_default()
            .split(':')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect()
    }

    pub fn get_sample_list(&self) -> Vec<String> {
        let mut list = HashSet::new();
        let mut dirs = vec![Self::samples_dir()];
        dirs.extend(Self::extra_forms_dirs());

        for dir in dirs {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            let ext_lower = ext.to_ascii_lowercase();
                            if matches!(ext_lower.as_str(), "xml" | "fmb" | "mmb" | "olb" | "pll" | "pld" | "rdf") {
                                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                    list.insert(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut sorted: Vec<String> = list.into_iter().collect();
        sorted.sort();
        sorted
    }

    /// Loads a module from `samples/` or `OFW_FORMS_PATH_EXTRA` directories.
    /// Only plain file names are accepted, so a request cannot open arbitrary paths.
    pub fn load_sample(&self, sample_name: &str) -> Result<ModuleIndex> {
        if !is_plain_file_name(sample_name) {
            return Err(FormsError::NotFound(format!("Invalid sample name '{}'", sample_name)));
        }
        let mut search_dirs = vec![Self::samples_dir()];
        search_dirs.extend(Self::extra_forms_dirs());
        for dir in search_dirs {
            if let Some(path) = safe_join(&dir, sample_name) {
                if path.is_file() {
                    return self.load_module(&path);
                }
            }
        }
        Err(FormsError::NotFound(format!("Sample form '{}' not found in samples or OFW_FORMS_PATH_EXTRA directories", sample_name)))
    }

    pub fn load_module(&self, file_path: &Path) -> Result<ModuleIndex> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| FormsError::InvalidFormat("Invalid file name".to_string()))?;

        let key = ModuleKey::from_file_name(file_name)
            .unwrap_or_else(|| ModuleKey::new("UNKNOWN", ModuleType::Form));

        let target_cache_dir = self.base_cache_dir.join(file_name.replace('.', "_"));
        fs::create_dir_all(&target_cache_dir)?;

        let lower_name = file_name.to_ascii_lowercase();
        let (mut index, effective_path) = if lower_name.ends_with(".pld") {
            (PldParser::parse(&key, file_path, &target_cache_dir)?, file_path.to_path_buf())
        } else if lower_name.ends_with(".xml") {
            (FormsXmlParser::parse(&key, file_path, &target_cache_dir)?, file_path.to_path_buf())
        } else {
            // Binary form (.fmb, .mmb, .olb)
            // Check if sibling XML exists in known candidate paths (matches Kotlin ModuleConverter)
            if let Some(sibling_xml) = Self::find_sibling_xml(file_path, &key) {
                tracing::info!("Resolved sibling XML for {}: {:?}", file_name, sibling_xml);
                (FormsXmlParser::parse(&key, &sibling_xml, &target_cache_dir)?, sibling_xml)
            } else {
                // Direct Binary parsing
                (BinaryFmbParser::parse(&key, file_path, &target_cache_dir)?, file_path.to_path_buf())
            }
        };

        // If form has an associated MenuModule and no menus yet, resolve companion menu XML
        if index.menus.is_empty() {
            if let Some(ref mm) = index.menu_module {
                if let Some(menu_xml) = Self::find_menu_xml(file_path, mm) {
                    tracing::info!("Resolved menu module XML for {}: {:?}", mm, menu_xml);
                    let menu_key = ModuleKey::new(mm, ModuleType::Menu);
                    if let Ok(menu_index) = FormsXmlParser::parse(&menu_key, &menu_xml, &target_cache_dir) {
                        index.menus = menu_index.menus;
                        for pu in menu_index.program_units {
                            if !index.program_units.iter().any(|existing| existing.name.eq_ignore_ascii_case(&pu.name)) {
                                index.program_units.push(pu);
                            }
                        }
                    }
                }
            }
        }

        // Resolve VisualAttribute, PropertyClass, and external ParentFilename/ParentModule inheritance
        resolve_module_inheritance(&mut index, &effective_path, &target_cache_dir);
        // Resolve TabPage -> Stacked / nested Tab canvas associations from WHEN-TAB-PAGE-CHANGED / ProgramUnits
        resolve_tab_page_associations(&mut index, &target_cache_dir);

        let mut st = self.state.write().unwrap();
        let key_name = key.name.to_ascii_uppercase();
        st.modules.insert(key_name.clone(), index.clone());
        st.source_files.insert(key_name.clone(), file_path.to_path_buf());
        st.converted_files.insert(key_name.clone(), effective_path.clone());
        if let Some(base) = lower_name.strip_suffix("_report.xml") {
            st.converted_files.insert(base.to_ascii_uppercase(), effective_path);
        }
        st.cache_dirs.insert(key_name, target_cache_dir);

        Ok(index)
    }

    fn find_sibling_xml(file_path: &Path, key: &ModuleKey) -> Option<PathBuf> {
        let base_name = &key.name;
        let ext_type = key.module_type.extension();
        let candidates = [
            format!("{}_{}.xml", base_name.to_ascii_lowercase(), ext_type),
            format!("{}_{}.xml", base_name.to_ascii_uppercase(), ext_type.to_ascii_uppercase()),
            format!("{}_{}.XML", base_name.to_ascii_uppercase(), ext_type.to_ascii_uppercase()),
            format!("{}.xml", base_name.to_ascii_lowercase()),
            format!("{}.xml", base_name.to_ascii_uppercase()),
            format!("{}.XML", base_name.to_ascii_uppercase()),
        ];

        let mut search_dirs = Vec::new();
        if let Some(parent) = file_path.parent() {
            if parent.as_os_str().is_empty() {
                search_dirs.push(PathBuf::from("."));
            } else {
                search_dirs.push(parent.to_path_buf());
            }
        }
        search_dirs.push(Self::samples_dir());

        for dir in search_dirs {
            if dir.is_dir() {
                for cand in &candidates {
                    let p = dir.join(cand);
                    if p.is_file() {
                        return Some(p);
                    }
                }
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let fname = entry.file_name().to_string_lossy().to_string();
                        for cand in &candidates {
                            if fname.eq_ignore_ascii_case(cand) {
                                let p = entry.path();
                                if p.is_file() {
                                    return Some(p);
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn find_menu_xml(file_path: &Path, menu_module_name: &str) -> Option<PathBuf> {
        let base_name = menu_module_name.trim();
        let candidates = [
            format!("{}_mmb.xml", base_name.to_ascii_lowercase()),
            format!("{}_mmb.xml", base_name.to_ascii_uppercase()),
            format!("{}.mmb.xml", base_name.to_ascii_lowercase()),
            format!("{}.xml", base_name.to_ascii_lowercase()),
            format!("{}.xml", base_name.to_ascii_uppercase()),
        ];

        let mut search_dirs = Vec::new();
        if let Some(parent) = file_path.parent() {
            if parent.as_os_str().is_empty() {
                search_dirs.push(PathBuf::from("."));
            } else {
                search_dirs.push(parent.to_path_buf());
            }
        }
        search_dirs.push(Self::samples_dir());

        for dir in search_dirs {
            if dir.is_dir() {
                for cand in &candidates {
                    let p = dir.join(cand);
                    if p.is_file() {
                        return Some(p);
                    }
                }
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let fname = entry.file_name().to_string_lossy().to_string();
                        for cand in &candidates {
                            if fname.eq_ignore_ascii_case(cand) {
                                let p = entry.path();
                                if p.is_file() {
                                    return Some(p);
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub fn get_module(&self, module_name: &str) -> Option<ModuleIndex> {
        let st = self.state.read().unwrap();
        st.modules.get(&module_name.to_ascii_uppercase()).cloned()
    }

    pub async fn query_block(
        &self,
        module_name: &str,
        block_name: Option<&str>,
        config: &oracle_forms_db::DbConfig,
        limit: Option<usize>,
    ) -> Result<oracle_forms_db::QueryResult> {
        let module = match self.get_module(module_name) {
            Some(m) => m,
            None => {
                self.load_sample(module_name)
                    .or_else(|_| self.load_sample(&format!("{}.xml", module_name)))
                    .or_else(|_| self.load_sample(&format!("{}_fmb.xml", module_name)))
                    .or_else(|_| self.load_sample(&format!("{}.fmb", module_name)))
                    .map_err(|_| FormsError::NotFound(format!("Module '{}' not loaded in session or samples", module_name)))?
            }
        };

        let block = match block_name {
            Some(b_name) => module
                .blocks
                .iter()
                .find(|b| b.name.eq_ignore_ascii_case(b_name))
                .ok_or_else(|| FormsError::NotFound(format!("Block '{}' not found in module", b_name)))?,
            None => module
                .blocks
                .iter()
                .find(|b| b.query_data_source_name.is_some() || !b.items.is_empty())
                .ok_or_else(|| FormsError::NotFound("No queryable blocks found in module".to_string()))?,
        };

        // No fallback to the mock driver: if the requested database is unreachable the
        // caller gets the error, never fake rows that look real.
        let active_config = self.apply_credential_policy(config.clone());
        let driver = oracle_forms_db::create_driver(&active_config)
            .map_err(|e| FormsError::Database(e.to_string()))?;
        driver
            .test_connection()
            .await
            .map_err(|e| FormsError::Database(e.to_string()))?;

        driver
            .query_block(block, limit)
            .await
            .map_err(|e| FormsError::Internal(e.to_string()))
    }

    /// Reads a sidecar source file for a loaded module. `relative_path` comes from the
    /// request, so it is confined to the module's cache directory (or the directory of the
    /// module file itself) via [`safe_join`].
    pub fn read_source(&self, module_name: &str, relative_path: &str) -> Option<String> {
        // Absolute paths are rejected by `safe_join`, never reinterpreted as relative.
        let clean_path = relative_path;
        let key = module_name.to_ascii_uppercase();
        let st = self.state.read().unwrap();

        if let Some(cache_dir) = st.cache_dirs.get(&key) {
            if let Some(sidecar_file) = safe_join(cache_dir, clean_path) {
                if sidecar_file.is_file() {
                    if let Ok(text) = fs::read_to_string(&sidecar_file) {
                        return Some(text);
                    }
                }
            }
        }

        if let Some(direct_converted) = st.converted_files.get(&key) {
            if let Some(parent) = direct_converted.parent() {
                let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
                if let Some(converted_relative) = safe_join(parent, clean_path) {
                    if converted_relative.is_file() {
                        if let Ok(text) = fs::read_to_string(&converted_relative) {
                            return Some(text);
                        }
                    }
                }
            }
        }

        None
    }

    pub fn get_source_file(&self, module_name: &str) -> Option<PathBuf> {
        let key = module_name.to_ascii_uppercase();
        self.state.read().unwrap().source_files.get(&key).cloned()
    }

    pub fn read_xml_slice(&self, module_name: &str, start_line: usize, end_line: usize) -> Option<String> {
        let key = module_name.to_ascii_uppercase();
        let st = self.state.read().unwrap();
        let file = st.converted_files.get(&key)?;

        if !file.is_file() {
            return None;
        }

        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.to_ascii_lowercase().ends_with(".fmb") {
            let file_size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
            return Some(format!(
                "<!-- This module was parsed in Direct Binary .FMB mode.\n     Source file: {} ({} bytes)\n     All PL/SQL triggers, program units, and form objects were extracted directly from the binary.\n     To inspect raw XML elements, convert this module with Oracle's frmf2xml utility. -->",
                file_name, file_size
            ));
        }

        let content = fs::read_to_string(file).ok()?;
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Some(String::new());
        }

        let s = start_line.saturating_sub(1).min(lines.len().saturating_sub(1));
        let e = end_line.saturating_sub(1).min(lines.len().saturating_sub(1)).max(s);

        Some(lines[s..=e].join("\n"))
    }

    pub fn resolve_effective_item(&self, module_name: &str, item: &ItemInfo) -> EffectiveItemProperties {
        let index = match self.get_module(module_name) {
            Some(idx) => idx,
            None => {
                return EffectiveItemProperties {
                    width: item.width,
                    height: item.height,
                    x_position: item.x_position,
                    y_position: item.y_position,
                    prompt: item.prompt.clone(),
                    item_type: item.item_type.clone(),
                    dml: item.dml.clone(),
                    resolved_through: Vec::new(),
                }
            }
        };

        let mut eff_width = item.width;
        let mut eff_height = item.height;
        let eff_prompt = item.prompt.clone();
        let eff_type = item.item_type.clone();
        let mut eff_dml = item.dml.clone().unwrap_or_default();
        let mut through = Vec::new();

        if let Some(ref class_name) = item.property_class {
            let mut visited = HashSet::new();
            let chain = resolve_class_chain(&index, class_name, &mut visited, 0);
            through.extend(chain.through);
            if eff_width.is_none() {
                eff_width = chain.geometry.width;
            }
            if eff_height.is_none() {
                eff_height = chain.geometry.height;
            }
            eff_dml = eff_dml.over(&chain.dml);
        }

        EffectiveItemProperties {
            width: eff_width,
            height: eff_height,
            x_position: item.x_position,
            y_position: item.y_position,
            prompt: eff_prompt,
            item_type: eff_type,
            dml: Some(eff_dml),
            resolved_through: through,
        }
    }

    pub fn create_module(&self, module: ModuleIndex) -> bool {
        let mut state = self.state.write().unwrap();
        let key = module.key.name.to_ascii_uppercase();
        if state.modules.contains_key(&key) { return false; }
        state.modules.insert(key, module);
        true
    }

    #[allow(dead_code)]
    pub fn save_module(&self, mut module: ModuleIndex) {
        let mut st = self.state.write().unwrap();
        let key = module.key.name.to_ascii_uppercase();
        if let Some(previous) = st.modules.get(&key) {
            // Preserve the server-side source_file so client sync cannot redirect saves into samples/.
            if !previous.source_file.is_empty() {
                module.source_file = previous.source_file.clone();
            }
            if previous != &module {
                st.model_edited.insert(key.clone());
            }
        }
        st.modules.insert(key, module);
    }

    #[allow(clippy::too_many_arguments, dead_code)]
    pub fn update_item(
        &self,
        module_name: &str,
        block_name: &str,
        item_name: &str,
        x: Option<i32>,
        y: Option<i32>,
        width: Option<i32>,
        height: Option<i32>,
        prompt: Option<String>,
        label: Option<String>,
    ) -> Result<ModuleIndex> {
        self.update_item_ext(
            module_name,
            crate::routes::UpdateItemRequest {
                block_name: block_name.to_string(),
                item_name: item_name.to_string(),
                x,
                y,
                width,
                height,
                prompt,
                label,
                ..Default::default()
            },
        )
    }

    pub fn update_item_ext(
        &self,
        module_name: &str,
        req: crate::routes::UpdateItemRequest,
    ) -> Result<ModuleIndex> {
        let mod_upper = module_name.to_ascii_uppercase();
        if !self.state.read().unwrap().modules.contains_key(&mod_upper) {
            let loaded = self.get_or_load_module(module_name)?;
            self.save_module(loaded);
        }

        let mut st = self.state.write().unwrap();
        let module = st
            .modules
            .get_mut(&mod_upper)
            .ok_or_else(|| FormsError::NotFound(format!("Module '{}' not found in service state", module_name)))?;

        let block = module
            .blocks
            .iter_mut()
            .find(|b| b.name.eq_ignore_ascii_case(&req.block_name))
            .ok_or_else(|| FormsError::NotFound(format!("Block '{}' not found in module '{}'", req.block_name, module_name)))?;

        let item = block
            .items
            .iter_mut()
            .find(|i| i.name.eq_ignore_ascii_case(&req.item_name))
            .ok_or_else(|| FormsError::NotFound(format!("Item '{}' not found in block '{}'", req.item_name, req.block_name)))?;

        if let Some(x_val) = req.x { item.x_position = x_val; }
        if let Some(y_val) = req.y { item.y_position = y_val; }
        if let Some(w_val) = req.width { item.width = Some(w_val); }
        if let Some(h_val) = req.height { item.height = Some(h_val); }
        if let Some(ref v) = req.prompt { item.prompt = Some(v.clone()); }
        if let Some(ref v) = req.label { item.label = Some(v.clone()); }
        if let Some(ref v) = req.item_type { item.item_type = Some(v.clone()); }
        if let Some(ref v) = req.data_type { item.data_type = Some(v.clone()); }
        if let Some(ref v) = req.column_name { item.column_name = Some(v.clone()); }
        if let Some(ref v) = req.back_color {
            item.back_color = if v.trim().is_empty() { None } else { Some(v.clone()) };
        }
        if let Some(ref v) = req.fore_color {
            item.fore_color = if v.trim().is_empty() { None } else { Some(v.clone()) };
        }
        if let Some(v) = req.enabled {
            item.enabled = v;
            item.dml.get_or_insert_with(Default::default).enabled = Some(v);
        }
        if let Some(v) = req.visible { item.visible = Some(v); }
        if let Some(v) = req.required {
            item.required = Some(v);
            item.dml.get_or_insert_with(Default::default).required = Some(v);
        }
        if let Some(ref v) = req.prompt_attachment_edge { item.prompt_attachment_edge = v.clone(); }
        if let Some(v) = req.prompt_attachment_offset { item.prompt_attachment_offset = v; }
        if let Some(ref v) = req.initial_value {
            item.dml.get_or_insert_with(Default::default).initial_value = Some(v.clone());
        }
        if let Some(v) = req.maximum_length {
            item.dml.get_or_insert_with(Default::default).maximum_length = Some(v);
        }
        let updated = module.clone();

        // Record the edit as attribute changes for a lossless export.
        let mut attrs: Vec<(String, String)> = Vec::new();
        if let Some(v) = req.x { attrs.push(("XPosition".into(), v.to_string())); }
        if let Some(v) = req.y { attrs.push(("YPosition".into(), v.to_string())); }
        if let Some(v) = req.width { attrs.push(("Width".into(), v.to_string())); }
        if let Some(v) = req.height { attrs.push(("Height".into(), v.to_string())); }
        if let Some(v) = req.prompt { attrs.push(("Prompt".into(), v)); }
        if let Some(v) = req.label { attrs.push(("Label".into(), v)); }
        if let Some(v) = req.item_type { attrs.push(("ItemType".into(), v)); }
        if let Some(v) = req.data_type { attrs.push(("DataType".into(), v)); }
        if let Some(v) = req.column_name { attrs.push(("ColumnName".into(), v)); }
        if let Some(v) = req.back_color { attrs.push(("BackColor".into(), v)); }
        if let Some(v) = req.fore_color { attrs.push(("ForegroundColor".into(), v)); }
        if let Some(v) = req.enabled { attrs.push(("Enabled".into(), if v { "true" } else { "false" }.into())); }
        if let Some(v) = req.visible { attrs.push(("Visible".into(), if v { "true" } else { "false" }.into())); }
        if let Some(v) = req.required { attrs.push(("Required".into(), if v { "true" } else { "false" }.into())); }
        if let Some(v) = req.prompt_attachment_edge { attrs.push(("PromptAttachmentEdge".into(), v)); }
        if let Some(v) = req.prompt_attachment_offset { attrs.push(("PromptAttachmentOffset".into(), v.to_string())); }
        if let Some(v) = req.initial_value { attrs.push(("InitializeValue".into(), v)); }
        if let Some(v) = req.maximum_length { attrs.push(("MaximumLength".into(), v.to_string())); }
        let edits = st.item_edits.entry(mod_upper).or_default();
        match edits.iter_mut().find(|p| p.block.eq_ignore_ascii_case(&req.block_name) && p.item.eq_ignore_ascii_case(&req.item_name)) {
            Some(existing) => {
                for (k, v) in attrs {
                    match existing.attributes.iter_mut().find(|(ek, _)| *ek == k) {
                        Some(slot) => slot.1 = v,
                        None => existing.attributes.push((k, v)),
                    }
                }
            }
            None => edits.push(oracle_forms_xml::ItemPatch { block: req.block_name, item: req.item_name, attributes: attrs }),
        }

        Ok(updated)
    }

    /// Finds and parses an attached library (`.pld`) for a module: looks next to the
    /// module file, then in the samples directory. Only plain file names are tried.
    pub fn resolve_library(&self, module_name: &str, library_name: &str) -> Option<ModuleIndex> {
        let base = std::path::Path::new(library_name.trim())
            .file_stem()
            .and_then(|s| s.to_str())?
            .to_string();
        if !is_plain_file_name(&base) {
            return None;
        }
        let mut dirs = Vec::new();
        {
            let st = self.state.read().unwrap();
            if let Some(parent) = st.converted_files.get(&module_name.to_ascii_uppercase()).and_then(|f| f.parent()) {
                dirs.push(if parent.as_os_str().is_empty() { PathBuf::from(".") } else { parent.to_path_buf() });
            }
        }
        dirs.push(Self::samples_dir());
        let candidates = [format!("{}.pld", base.to_ascii_lowercase()), format!("{}.pld", base.to_ascii_uppercase()), format!("{}.pld", base)];
        for dir in dirs {
            for cand in &candidates {
                if let Some(path) = safe_join(&dir, cand) {
                    if path.is_file() {
                        let key = ModuleKey::new(base.to_ascii_uppercase(), ModuleType::Library);
                        let cache = self.base_cache_dir.join(format!("lib_{}", base.to_ascii_lowercase()));
                        let _ = fs::create_dir_all(&cache);
                        if let Ok(idx) = PldParser::parse(&key, &path, &cache) {
                            return Some(idx);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn get_or_load_module(&self, name: &str) -> Result<ModuleIndex> {
        if let Some(m) = self.get_module(name) {
            return Ok(m);
        }
        self.load_sample(name)
            .or_else(|_| self.load_sample(&format!("{}.xml", name)))
            .or_else(|_| self.load_sample(&format!("{}_fmb.xml", name)))
    }

    /// Exports a module as Forms2XML. If the module was opened from an XML file, the
    /// original file is returned with only the edited attributes changed (lossless).
    /// Otherwise (binary .fmb) a summary is regenerated from the parsed model.
    pub fn export_xml(&self, module_name: &str) -> Result<(String, ExportMode)> {
        let module = self.get_or_load_module(module_name)?;
        let key = module.key.name.to_ascii_uppercase();
        if self.state.read().unwrap().model_edited.contains(&key) {
            return Ok((Self::summary_xml(&module), ExportMode::Summary));
        }
        let (source, edits) = {
            let st = self.state.read().unwrap();
            (
                st.converted_files.get(&key).cloned(),
                st.item_edits.get(&key).cloned().unwrap_or_default(),
            )
        };
        if let Some(path) = source.filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("xml")).unwrap_or(false)) {
            if let Ok(original) = fs::read_to_string(&path) {
                let patched = oracle_forms_xml::patch_items(&original, &edits)
                    .map_err(|e| FormsError::InvalidFormat(e.to_string()))?;
                return Ok((patched, ExportMode::PatchedOriginal));
            }
        }
        Ok((Self::summary_xml(&module), ExportMode::Summary))
    }

    /// Directory for saved forms that did not originate from `samples/` (such as uploaded binaries): `saved/` in
    /// the work folder (`OFW_WORK_DIR`), which is outside the repository so nothing can be committed by accident.
    pub fn temp_saved_dir() -> PathBuf {
        let root = Self::samples_dir().canonicalize().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_else(|| PathBuf::from("."));
        crate::oracle_preview::work_dir(&root).join("saved")
    }

    fn is_in_samples_dir(path: &Path) -> bool {
        let samples = Self::samples_dir();
        if let (Ok(canon_path), Ok(canon_samples)) = (path.canonicalize(), samples.canonicalize()) {
            return canon_path.is_file() && canon_path.starts_with(canon_samples);
        }
        false
    }

    fn resolve_save_destination(source_file: &str, extra_dirs: &[PathBuf]) -> (PathBuf, String) {
        let source_path = Path::new(source_file);
        if Self::is_in_samples_dir(source_path) {
            return (Self::samples_dir(), "samples".to_string());
        }
        if let Ok(canon_source) = source_path.canonicalize() {
            if let Some(matching_extra) = extra_dirs.iter().find(|dir| {
                dir.canonicalize().is_ok_and(|canon_dir| canon_source.starts_with(canon_dir))
            }) {
                return (matching_extra.clone(), matching_extra.to_string_lossy().to_string());
            }
        }
        if let Some(first_extra) = extra_dirs.first() {
            return (first_extra.clone(), first_extra.to_string_lossy().to_string());
        }
        let saved = Self::temp_saved_dir();
        let display = saved.to_string_lossy().to_string();
        (saved, display)
    }

    /// Saves the form in place: into `samples/` if it originated from `samples/`, into the matching
    /// (or first) `OFW_FORMS_PATH_EXTRA` directory for external/uploaded forms, or `<work folder>/saved/` (outside the repository)
    /// when `OFW_FORMS_PATH_EXTRA` is not configured.
    pub fn save_form_file(&self, module_name: &str) -> Result<(PathBuf, String, String, ExportMode)> {
        let module = self.get_or_load_module(module_name)?;
        if module.key.module_type != ModuleType::Form {
            return Err(FormsError::InvalidFormat("Save Form requires a form module".into()));
        }
        let stem = module.key.name.to_ascii_lowercase();
        if stem.is_empty() || !stem.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            return Err(FormsError::InvalidFormat("Form names must contain letters, digits or underscores".into()));
        }
        let extra_dirs = Self::extra_forms_dirs();
        let (target_dir, prefix) = Self::resolve_save_destination(&module.source_file, &extra_dirs);
        let (xml, mode) = self.export_xml(module_name)?;
        let name = Self::write_form_file(&target_dir, &stem, &xml)?;
        let saved_path = target_dir.join(&name);
        let display_path = format!("{}/{}", prefix.trim_end_matches('/'), name);
        Ok((saved_path, name, display_path, mode))
    }

    /// Updates `<stem>_fmb.xml` in place via a temporary file and a rename.
    fn write_form_file(directory: &Path, stem: &str, xml: &str) -> Result<String> {
        use std::io::Write;
        fs::create_dir_all(directory)?;
        let name = format!("{}_fmb.xml", stem);
        let path = directory.join(&name);
        let temp = directory.join(format!(".{}.saving", name));
        let result = fs::File::create(&temp)
            .and_then(|mut file| file.write_all(xml.as_bytes()).and_then(|_| file.sync_all()))
            .and_then(|_| fs::rename(&temp, &path));
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(error.into());
        }
        Ok(name)
    }

    /// Regenerated (lossy) XML, clearly labelled as such.
    pub fn summary_xml(module: &ModuleIndex) -> String {
        let xml = oracle_forms_xml::FormsXmlWriter::write_string(module);
        let note = "<!-- SUMMARY EXPORT: regenerated from the Workbench's parsed model. Properties the \
Workbench does not model (fonts, LOV and record-group details, most item \
properties) are omitted. Do not compile this over the original form; convert the .fmb with \
frmf2xml for a complete export. -->\n";
        match xml.find("?>") {
            Some(i) => format!("{}\n{}{}", &xml[..i + 2], note, xml[i + 2..].trim_start()),
            None => format!("{}{}", note, xml),
        }
    }

    #[allow(dead_code)]
    pub fn diff_modules(&self, base_name: &str, modified_name: &str) -> Result<oracle_forms_core::diff::ModuleDiff> {
        let base = self.get_or_load_module(base_name)?;
        let modified = self.get_or_load_module(modified_name)?;
        Ok(oracle_forms_core::diff::FormDiffer::diff(&base, &modified))
    }

    pub fn get_report_samples(&self) -> Vec<ReportSampleItem> {
        let mut list = Vec::new();
        let samples_dir = Self::samples_dir();
        if let Ok(entries) = fs::read_dir(&samples_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if file_name.ends_with("_report.xml") || file_name.ends_with("_rdf.xml") {
                        if let Ok(report) = oracle_forms_xml::ReportsXmlParser::parse_file(&path) {
                            list.push(ReportSampleItem {
                                name: report.name.clone(),
                                file_name: file_name.to_string(),
                                title: report.title.unwrap_or(report.name),
                            });
                        }
                    }
                }
            }
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    /// Loads a report definition from the samples directory by plain name.
    pub fn load_report(&self, name_or_file: &str) -> Result<oracle_forms_core::report::ReportDefinition> {
        if !is_plain_file_name(name_or_file) {
            return Err(FormsError::NotFound(format!("Invalid report name '{}'", name_or_file)));
        }
        let samples_dir = Self::samples_dir();
        let candidates = [
            name_or_file.to_string(),
            format!("{}.xml", name_or_file),
            format!("{}_report.xml", name_or_file.to_ascii_lowercase()),
            format!("{}_report.xml", name_or_file.to_ascii_uppercase()),
        ];

        for candidate in &candidates {
            if let Some(path) = safe_join(&samples_dir, candidate) {
                if path.is_file() {
                    let report = oracle_forms_xml::ReportsXmlParser::parse_file(&path)?;
                    let mut st = self.state.write().unwrap();
                    st.converted_files.insert(report.name.to_ascii_uppercase(), path);
                    return Ok(report);
                }
            }
        }

        Err(FormsError::NotFound(format!("Report definition '{}' not found", name_or_file)))
    }

    pub async fn run_report(
        &self,
        name_or_file: &str,
        backend_str: &str,
        params: &HashMap<String, String>,
    ) -> Result<(oracle_forms_core::report::ReportDefinition, oracle_forms_runtime::ReportExecutionResult, String)> {
        let report = self.load_report(name_or_file)?;

        let backend = oracle_forms_db::DbBackend::parse(backend_str)
            .map_err(|e| FormsError::Database(e.to_string()))?;

        let config = self.apply_credential_policy(oracle_forms_db::DbConfig {
            backend,
            ..Default::default()
        });

        // No silent fallback to mock data when the requested database is unreachable.
        let driver = oracle_forms_db::create_driver(&config)
            .map_err(|e| FormsError::Database(e.to_string()))?;
        driver
            .test_connection()
            .await
            .map_err(|e| FormsError::Database(e.to_string()))?;

        let result = oracle_forms_runtime::ReportDataEvaluator::evaluate(&report, &*driver, params).await?;
        let html = oracle_forms_runtime::ReportRenderer::render_html(&report, &result);

        Ok((report, result, html))
    }

    /// Attaches or switches a custom MenuModule (.mmb / _mmb.xml) on an open form module.
    pub fn attach_menu(&self, module_name: &str, menu_name_or_file: &str) -> Result<ModuleIndex> {
        let mut index = self.get_or_load_module(module_name)?;
        let key_upper = index.key.name.to_ascii_uppercase();
        let target_cache_dir = {
            let st = self.state.read().unwrap();
            st.cache_dirs
                .get(&key_upper)
                .cloned()
                .unwrap_or_else(|| self.base_cache_dir.join(format!("{}_cache", key_upper)))
        };
        let _ = fs::create_dir_all(&target_cache_dir);

        if menu_name_or_file.trim().is_empty() || menu_name_or_file.eq_ignore_ascii_case("DEFAULT") {
            index.menu_module = None;
            index.menus.clear();
        } else {
            let samples_dir = Self::samples_dir();
            let menu_xml = Self::find_menu_xml(&samples_dir.join("dummy.fmb"), menu_name_or_file)
                .or_else(|| safe_join(&samples_dir, menu_name_or_file).filter(|p| p.is_file()));

            let Some(menu_path) = menu_xml else {
                return Err(FormsError::NotFound(format!("Menu module '{}' not found", menu_name_or_file)));
            };

            let menu_key = ModuleKey::from_file_name(
                menu_path.file_name().and_then(|s| s.to_str()).unwrap_or(menu_name_or_file),
            )
            .unwrap_or_else(|| ModuleKey::new(menu_name_or_file, ModuleType::Menu));

            let menu_index = if menu_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.eq_ignore_ascii_case("xml"))
                .unwrap_or(false)
            {
                FormsXmlParser::parse(&menu_key, &menu_path, &target_cache_dir)?
            } else if let Some(sib) = Self::find_sibling_xml(&menu_path, &menu_key) {
                FormsXmlParser::parse(&menu_key, &sib, &target_cache_dir)?
            } else {
                BinaryFmbParser::parse(&menu_key, &menu_path, &target_cache_dir)?
            };

            index.menu_module = Some(menu_index.key.name.clone());
            index.menus = menu_index.menus;
            for pu in menu_index.program_units {
                if !index.program_units.iter().any(|existing| existing.name.eq_ignore_ascii_case(&pu.name)) {
                    index.program_units.push(pu);
                }
            }
        }

        let mut st = self.state.write().unwrap();
        st.modules.insert(key_upper, index.clone());
        Ok(index)
    }

    /// Lists browsable Oracle Forms & Reports artifacts across `samples/` and `samples/conformance/`.
    pub fn browse_workspace_files(&self) -> Vec<BrowsedFileItem> {
        let mut items = Vec::new();
        let samples_dir = Self::samples_dir();
        let mut dirs = vec![samples_dir.clone()];
        let conf_dir = samples_dir.join("conformance");
        if conf_dir.is_dir() {
            dirs.push(conf_dir);
        }

        for dir in dirs {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let lower = fname.to_ascii_lowercase();
                    let category = if lower.ends_with("_report.xml")
                        || lower.ends_with("_rdf.xml")
                        || lower.ends_with(".rdf")
                        || lower.ends_with(".rpt")
                        || lower.ends_with(".rex")
                    {
                        "REPORT"
                    } else if lower.ends_with(".mmb") || lower.ends_with("_mmb.xml") {
                        "MENU"
                    } else if lower.ends_with(".olb") || lower.ends_with("_olb.xml") {
                        "OBJECT_LIB"
                    } else if lower.ends_with(".pld") || lower.ends_with(".pll") {
                        "PLSQL_LIB"
                    } else if lower.ends_with(".fmb") || lower.ends_with("_fmb.xml") || lower.ends_with(".xml") {
                        "FORM"
                    } else {
                        continue;
                    };

                    let size_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let rel_path = if dir.ends_with("conformance") {
                        format!("samples/conformance/{}", fname)
                    } else {
                        format!("samples/{}", fname)
                    };

                    items.push(BrowsedFileItem {
                        file_name: fname.to_string(),
                        relative_path: rel_path,
                        category: category.to_string(),
                        size_bytes,
                    });
                }
            }
        }

        items.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.file_name.cmp(&b.file_name)));
        items
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsedFileItem {
    pub file_name: String,
    pub relative_path: String,
    pub category: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReportSampleItem {
    pub name: String,
    pub file_name: String,
    pub title: String,
}

struct ClassChain {
    dml: ItemDml,
    geometry: ItemGeometry,
    through: Vec<String>,
}

fn resolve_class_chain(
    index: &ModuleIndex,
    class_name: &str,
    visited: &mut HashSet<String>,
    hops: usize,
) -> ClassChain {
    let here = format!("{}:{}", index.key.name, class_name);
    let info = index.property_class_details.iter().find(|c| c.name.eq_ignore_ascii_case(class_name));

    if info.is_none() || hops >= 10 || !visited.insert(here.to_ascii_uppercase()) {
        return ClassChain {
            dml: ItemDml::default(),
            geometry: ItemGeometry::default(),
            through: vec![here],
        };
    }

    let info = info.unwrap();
    let own_dml = info.item.clone().unwrap_or_default();
    let own_size = info.geometry.clone().unwrap_or_default();

    let parent_chain = if let Some(ref parent_class) = info.property_class {
        resolve_class_chain(index, parent_class, visited, hops + 1)
    } else {
        ClassChain {
            dml: ItemDml::default(),
            geometry: ItemGeometry::default(),
            through: Vec::new(),
        }
    };

    let mut all_through = vec![here];
    for t in parent_chain.through {
        if !all_through.contains(&t) {
            all_through.push(t);
        }
    }

    ClassChain {
        dml: own_dml.over(&parent_chain.dml),
        geometry: own_size.over(&parent_chain.geometry),
        through: all_through,
    }
}

fn inheritance_stem(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let file_part = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let stem = file_part
        .strip_suffix(".olb")
        .or_else(|| file_part.strip_suffix(".OLB"))
        .or_else(|| file_part.strip_suffix(".fmb"))
        .or_else(|| file_part.strip_suffix(".FMB"))
        .or_else(|| file_part.strip_suffix(".xml"))
        .or_else(|| file_part.strip_suffix(".XML"))
        .unwrap_or(file_part);
    if stem.is_empty() { None } else { Some(stem.to_string()) }
}

fn find_parent_xml(source_path: &Path, inh: &InheritanceRef) -> Option<PathBuf> {
    let raw_key = inh.file.as_deref().or(inh.module.as_deref())?;
    let stem = inheritance_stem(raw_key)?;
    let lower = stem.to_ascii_lowercase();
    let upper = stem.to_ascii_uppercase();
    let candidates = [
        format!("{}_olb.xml", lower),
        format!("{}_OLB.xml", upper),
        format!("{}.olb.xml", lower),
        format!("{}_fmb.xml", lower),
        format!("{}_FMB.xml", upper),
        format!("{}.fmb.xml", lower),
        format!("{}.xml", lower),
        format!("{}.xml", upper),
    ];

    let mut search_dirs = Vec::new();
    if let Some(parent) = source_path.parent() {
        if parent.as_os_str().is_empty() {
            search_dirs.push(PathBuf::from("."));
        } else {
            search_dirs.push(parent.to_path_buf());
        }
    }
    let samples = FormsService::samples_dir();
    if !search_dirs.contains(&samples) {
        search_dirs.push(samples);
    }
    for extra in FormsService::extra_forms_dirs() {
        if !search_dirs.contains(&extra) {
            search_dirs.push(extra);
        }
    }

    for dir in search_dirs {
        if !dir.is_dir() {
            continue;
        }
        for cand in &candidates {
            let p = dir.join(cand);
            if p.is_file() {
                return Some(p);
            }
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if candidates.iter().any(|c| fname.eq_ignore_ascii_case(c)) {
                    let p = entry.path();
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn load_parent_modules(index: &ModuleIndex, source_path: &Path, cache_dir: &Path) -> HashMap<String, ModuleIndex> {
    let mut loaded: HashMap<String, ModuleIndex> = HashMap::new();
    let mut queue: Vec<InheritanceRef> = Vec::new();

    for b in &index.blocks {
        if let Some(ref inh) = b.inherited { queue.push(inh.clone()); }
        for it in &b.items {
            if let Some(ref inh) = it.inherited { queue.push(inh.clone()); }
        }
    }
    for c in &index.canvases {
        if let Some(ref inh) = c.inherited { queue.push(inh.clone()); }
    }
    for g in &index.graphics {
        if let Some(ref inh) = g.inherited { queue.push(inh.clone()); }
    }
    for pc in &index.property_class_details {
        if let Some(ref inh) = pc.inherited { queue.push(inh.clone()); }
    }
    for va in &index.visual_attribute_details {
        if let Some(ref inh) = va.inherited { queue.push(inh.clone()); }
    }

    while let Some(inh) = queue.pop() {
        if loaded.len() >= 10 {
            break;
        }
        let key_opt = inh.file.as_deref().or(inh.module.as_deref()).and_then(inheritance_stem);
        let Some(stem) = key_opt else { continue };
        let key = stem.to_ascii_uppercase();
        if loaded.contains_key(&key) || key.eq_ignore_ascii_case(&index.key.name) {
            continue;
        }
        if let Some(parent_path) = find_parent_xml(source_path, &inh) {
            let mtype = if parent_path.to_string_lossy().to_ascii_lowercase().contains("_olb") {
                ModuleType::ObjectLibrary
            } else {
                ModuleType::Form
            };
            let pkey = ModuleKey::new(&stem, mtype);
            if let Ok(parsed) = FormsXmlParser::parse(&pkey, &parent_path, cache_dir) {
                for pc in &parsed.property_class_details {
                    if let Some(ref p_inh) = pc.inherited { queue.push(p_inh.clone()); }
                }
                for va in &parsed.visual_attribute_details {
                    if let Some(ref v_inh) = va.inherited { queue.push(v_inh.clone()); }
                }
                loaded.insert(parsed.key.name.to_ascii_uppercase(), parsed.clone());
                loaded.entry(key).or_insert(parsed);
            }
        }
    }
    loaded
}

fn va_to_style(va: &VisualAttributeInfo) -> VisualStyle {
    VisualStyle {
        back_color: va.back_color.clone(),
        fore_color: va.fore_color.clone(),
        fill_pattern: va.fill_pattern.clone(),
        font_name: va.font_name.clone(),
        font_size: va.font_size,
        font_weight: va.font_weight.clone(),
        font_style: va.font_style.clone(),
        bevel: None,
        visual_attribute: None,
        prompt_fore_color: va.prompt_fore_color.clone(),
        prompt_font_name: va.prompt_font_name.clone(),
        prompt_font_size: va.prompt_font_size,
        prompt_font_weight: va.prompt_font_weight.clone(),
    }
}

fn resolve_va_style(
    index: &ModuleIndex,
    parents: &HashMap<String, ModuleIndex>,
    va_name: &str,
    visited: &mut HashSet<String>,
) -> Option<VisualStyle> {
    let key = format!("{}:{}", index.key.name, va_name).to_ascii_uppercase();
    if !visited.insert(key) {
        return None;
    }
    if let Some(va) = index.visual_attribute_details.iter().find(|v| v.name.eq_ignore_ascii_case(va_name)) {
        let mut style = va_to_style(va);
        if let Some(ref inh) = va.inherited {
            if let Some(p_mod) = find_parent_module(parents, inh) {
                let target = inh.name.as_deref().unwrap_or(va_name);
                if let Some(parent_style) = resolve_va_style(p_mod, parents, target, visited) {
                    style = style.over(&parent_style);
                }
            }
        }
        return Some(style);
    }
    for p_mod in parents.values() {
        if let Some(va) = p_mod.visual_attribute_details.iter().find(|v| v.name.eq_ignore_ascii_case(va_name)) {
            return Some(va_to_style(va));
        }
    }
    None
}

fn find_parent_module<'a>(parents: &'a HashMap<String, ModuleIndex>, inh: &InheritanceRef) -> Option<&'a ModuleIndex> {
    for raw in [inh.file.as_deref(), inh.module.as_deref()].into_iter().flatten() {
        if let Some(stem) = inheritance_stem(raw) {
            if let Some(m) = parents.get(&stem.to_ascii_uppercase()) {
                return Some(m);
            }
        }
    }
    None
}

struct ResolvedPc {
    style: VisualStyle,
    geometry: ItemGeometry,
    dml: ItemDml,
    item_type: Option<String>,
    prompt: Option<String>,
    label: Option<String>,
    prompt_attachment_edge: Option<String>,
    prompt_attachment_offset: Option<i32>,
    prompt_align: Option<String>,
}

fn resolve_pc_full(
    index: &ModuleIndex,
    parents: &HashMap<String, ModuleIndex>,
    pc_name: &str,
    visited: &mut HashSet<String>,
    hops: usize,
) -> Option<ResolvedPc> {
    let key = format!("{}:{}", index.key.name, pc_name).to_ascii_uppercase();
    if hops >= 10 || !visited.insert(key) {
        return None;
    }
    let pc = index.property_class_details.iter().find(|c| c.name.eq_ignore_ascii_case(pc_name))?;
    let mut style = pc.style.clone().unwrap_or_default();
    if let Some(ref va_name) = style.visual_attribute {
        if let Some(va_style) = resolve_va_style(index, parents, va_name, &mut HashSet::new()) {
            style = style.over(&va_style);
        }
    }
    let mut geom = pc.geometry.clone().unwrap_or_default();
    let mut dml = pc.item.clone().unwrap_or_default();
    let mut item_type = pc.item_type.clone();
    let mut prompt = pc.prompt.clone();
    let mut label = pc.label.clone();
    let mut edge = pc.prompt_attachment_edge.clone();
    let mut offset = pc.prompt_attachment_offset;
    let mut align = pc.prompt_align.clone();

    if let Some(ref parent_pc) = pc.property_class {
        if let Some(parent) = resolve_pc_full(index, parents, parent_pc, visited, hops + 1) {
            style = style.over(&parent.style);
            geom = geom.over(&parent.geometry);
            dml = dml.over(&parent.dml);
            item_type = item_type.or(parent.item_type);
            prompt = prompt.or(parent.prompt);
            label = label.or(parent.label);
            edge = edge.or(parent.prompt_attachment_edge);
            offset = offset.or(parent.prompt_attachment_offset);
            align = align.or(parent.prompt_align);
        }
    } else if let Some(ref inh) = pc.inherited {
        if let Some(p_mod) = find_parent_module(parents, inh) {
            let target = inh.name.as_deref().unwrap_or(pc_name);
            if let Some(parent) = resolve_pc_full(p_mod, parents, target, visited, hops + 1) {
                style = style.over(&parent.style);
                geom = geom.over(&parent.geometry);
                dml = dml.over(&parent.dml);
                item_type = item_type.or(parent.item_type);
                prompt = prompt.or(parent.prompt);
                label = label.or(parent.label);
                edge = edge.or(parent.prompt_attachment_edge);
                offset = offset.or(parent.prompt_attachment_offset);
                align = align.or(parent.prompt_align);
            }
        }
    }

    Some(ResolvedPc {
        style,
        geometry: geom,
        dml,
        item_type,
        prompt,
        label,
        prompt_attachment_edge: edge,
        prompt_attachment_offset: offset,
        prompt_align: align,
    })
}

fn apply_style_to_item(item: &mut ItemInfo, style: &VisualStyle) {
    if item.back_color.is_none() { item.back_color = style.back_color.clone(); }
    if item.fore_color.is_none() { item.fore_color = style.fore_color.clone(); }
    if item.fill_pattern.is_none() { item.fill_pattern = style.fill_pattern.clone(); }
    if item.font_name.is_none() { item.font_name = style.font_name.clone(); }
    if item.font_size.is_none() { item.font_size = style.font_size; }
    if item.font_weight.is_none() { item.font_weight = style.font_weight.clone(); }
    if item.font_style.is_none() { item.font_style = style.font_style.clone(); }
    if item.bevel.is_none() { item.bevel = style.bevel.clone(); }
    if item.visual_attribute.is_none() { item.visual_attribute = style.visual_attribute.clone(); }
    if item.prompt_fore_color.is_none() { item.prompt_fore_color = style.prompt_fore_color.clone(); }
    if item.prompt_font_name.is_none() { item.prompt_font_name = style.prompt_font_name.clone(); }
    if item.prompt_font_size.is_none() { item.prompt_font_size = style.prompt_font_size; }
    if item.prompt_font_weight.is_none() { item.prompt_font_weight = style.prompt_font_weight.clone(); }
}

fn find_parent_item<'a>(p_mod: &'a ModuleIndex, inh: &InheritanceRef) -> Option<&'a ItemInfo> {
    let target_name = inh.name.as_deref()?;
    let block_hint = inh.owner_path.as_deref().and_then(|p| p.split('.').next());
    if let Some(b_name) = block_hint {
        if let Some(blk) = p_mod.blocks.iter().find(|b| b.name.eq_ignore_ascii_case(b_name)) {
            if let Some(it) = blk.items.iter().find(|i| i.name.eq_ignore_ascii_case(target_name)) {
                return Some(it);
            }
        }
    }
    for blk in &p_mod.blocks {
        if let Some(it) = blk.items.iter().find(|i| i.name.eq_ignore_ascii_case(target_name)) {
            return Some(it);
        }
    }
    None
}

fn resolve_module_inheritance(index: &mut ModuleIndex, source_path: &Path, cache_dir: &Path) {
    let parents = load_parent_modules(index, source_path, cache_dir);
    let mod_snapshot = index.clone();

    for block in &mut index.blocks {
        for item in &mut block.items {
            // 1. Item's own VisualAttribute
            if let Some(ref va_name) = item.visual_attribute.clone() {
                if let Some(va_style) = resolve_va_style(&mod_snapshot, &parents, va_name, &mut HashSet::new()) {
                    apply_style_to_item(item, &va_style);
                }
            }
            // 2. Local PropertyClass
            if let Some(ref pc_name) = item.property_class.clone() {
                if let Some(pc) = resolve_pc_full(&mod_snapshot, &parents, pc_name, &mut HashSet::new(), 0) {
                    apply_style_to_item(item, &pc.style);
                    if item.width.is_none() { item.width = pc.geometry.width; }
                    if item.height.is_none() { item.height = pc.geometry.height; }
                    if item.item_type.is_none() { item.item_type = pc.item_type; }
                    if item.prompt.is_none() { item.prompt = pc.prompt; }
                    if item.label.is_none() { item.label = pc.label; }
                    if !item.prompt_edge_set {
                        if let Some(edge) = pc.prompt_attachment_edge { item.prompt_attachment_edge = edge; item.prompt_edge_set = true; }
                    }
                    if !item.prompt_offset_set {
                        if let Some(off) = pc.prompt_attachment_offset { item.prompt_attachment_offset = off; item.prompt_offset_set = true; }
                    }
                    if !item.prompt_align_set {
                        if let Some(align) = pc.prompt_align { item.prompt_align = align; item.prompt_align_set = true; }
                    }
                }
            }
            // 3. External ParentFilename / ParentModule (OLB or parent FMB)
            if let Some(ref inh) = item.inherited.clone() {
                if let Some(p_mod) = find_parent_module(&parents, inh) {
                    let target = inh.name.as_deref().unwrap_or(&item.name);
                    if let Some(p_item) = find_parent_item(p_mod, inh) {
                        if item.width.is_none() { item.width = p_item.width; }
                        if item.height.is_none() { item.height = p_item.height; }
                        if item.item_type.is_none() { item.item_type = p_item.item_type.clone(); }
                        if item.prompt.is_none() { item.prompt = p_item.prompt.clone(); }
                        if item.label.is_none() { item.label = p_item.label.clone(); }
                        if item.back_color.is_none() { item.back_color = p_item.back_color.clone(); }
                        if item.fore_color.is_none() { item.fore_color = p_item.fore_color.clone(); }
                        if item.fill_pattern.is_none() { item.fill_pattern = p_item.fill_pattern.clone(); }
                        if item.bevel.is_none() { item.bevel = p_item.bevel.clone(); }
                        if item.font_name.is_none() { item.font_name = p_item.font_name.clone(); }
                        if item.font_size.is_none() { item.font_size = p_item.font_size; }
                        if item.font_weight.is_none() { item.font_weight = p_item.font_weight.clone(); }
                        if item.font_style.is_none() { item.font_style = p_item.font_style.clone(); }
                        if item.radio_buttons.is_empty() && !p_item.radio_buttons.is_empty() {
                            item.radio_buttons = p_item.radio_buttons.clone();
                        }
                        if !item.prompt_edge_set && p_item.prompt_edge_set {
                            item.prompt_attachment_edge = p_item.prompt_attachment_edge.clone();
                            item.prompt_edge_set = true;
                        }
                        if !item.prompt_offset_set && p_item.prompt_offset_set {
                            item.prompt_attachment_offset = p_item.prompt_attachment_offset;
                            item.prompt_offset_set = true;
                        }
                        if !item.prompt_align_set && p_item.prompt_align_set {
                            item.prompt_align = p_item.prompt_align.clone();
                            item.prompt_align_set = true;
                        }
                        if item.prompt_align_offset == 0 && p_item.prompt_align_offset != 0 {
                            item.prompt_align_offset = p_item.prompt_align_offset;
                        }
                        if item.prompt_fore_color.is_none() { item.prompt_fore_color = p_item.prompt_fore_color.clone(); }
                        if item.prompt_font_name.is_none() { item.prompt_font_name = p_item.prompt_font_name.clone(); }
                        if item.prompt_font_size.is_none() { item.prompt_font_size = p_item.prompt_font_size; }
                        if item.prompt_font_weight.is_none() { item.prompt_font_weight = p_item.prompt_font_weight.clone(); }
                        if let Some(ref p_va) = p_item.visual_attribute {
                            if let Some(va_style) = resolve_va_style(p_mod, &parents, p_va, &mut HashSet::new()) {
                                apply_style_to_item(item, &va_style);
                            }
                        }
                    } else if let Some(pc) = resolve_pc_full(p_mod, &parents, target, &mut HashSet::new(), 0) {
                        apply_style_to_item(item, &pc.style);
                        if item.width.is_none() { item.width = pc.geometry.width; }
                        if item.height.is_none() { item.height = pc.geometry.height; }
                        if item.item_type.is_none() { item.item_type = pc.item_type; }
                        if item.prompt.is_none() { item.prompt = pc.prompt; }
                        if item.label.is_none() { item.label = pc.label; }
                    } else if let Some(va_style) = resolve_va_style(p_mod, &parents, target, &mut HashSet::new()) {
                        apply_style_to_item(item, &va_style);
                    }
                }
            }
            // Apply any newly inherited visual_attribute if colors are still unset
            if let Some(ref va_name) = item.visual_attribute.clone() {
                if let Some(va_style) = resolve_va_style(&mod_snapshot, &parents, va_name, &mut HashSet::new()) {
                    apply_style_to_item(item, &va_style);
                }
            }
        }
    }

    for canvas in &mut index.canvases {
        if let Some(ref va_name) = canvas.visual_attribute {
            if let Some(va_style) = resolve_va_style(&mod_snapshot, &parents, va_name, &mut HashSet::new()) {
                if canvas.back_color.is_none() { canvas.back_color = va_style.back_color; }
            }
        }
        if let Some(ref inh) = canvas.inherited {
            if let Some(p_mod) = find_parent_module(&parents, inh) {
                let target = inh.name.as_deref().unwrap_or(&canvas.name);
                if let Some(p_canvas) = p_mod.canvases.iter().find(|c| c.name.eq_ignore_ascii_case(target)) {
                    if canvas.back_color.is_none() { canvas.back_color = p_canvas.back_color.clone(); }
                    if canvas.viewport_width.is_none() { canvas.viewport_width = p_canvas.viewport_width; }
                    if canvas.viewport_height.is_none() { canvas.viewport_height = p_canvas.viewport_height; }
                }
            }
        }
    }

    for g in &mut index.graphics {
        if let Some(ref va_name) = g.visual_attribute {
            if let Some(va_style) = resolve_va_style(&mod_snapshot, &parents, va_name, &mut HashSet::new()) {
                if g.back_color.is_none() { g.back_color = va_style.back_color; }
                if g.fore_color.is_none() { g.fore_color = va_style.fore_color; }
                if g.fill_pattern.is_none() { g.fill_pattern = va_style.fill_pattern; }
                if g.font_name.is_none() { g.font_name = va_style.font_name; }
                if g.font_size.is_none() { g.font_size = va_style.font_size; }
                if g.font_weight.is_none() { g.font_weight = va_style.font_weight; }
            }
        }
        if let Some(ref inh) = g.inherited {
            if let Some(p_mod) = find_parent_module(&parents, inh) {
                let target = inh.name.as_deref().unwrap_or(&g.name);
                if let Some(p_g) = p_mod.graphics.iter().find(|pg| pg.name.eq_ignore_ascii_case(target)) {
                    if g.back_color.is_none() { g.back_color = p_g.back_color.clone(); }
                    if g.fore_color.is_none() { g.fore_color = p_g.fore_color.clone(); }
                    if g.edge_foreground_color.is_none() { g.edge_foreground_color = p_g.edge_foreground_color.clone(); }
                    if g.fill_pattern.is_none() { g.fill_pattern = p_g.fill_pattern.clone(); }
                    if g.bevel.is_none() { g.bevel = p_g.bevel.clone(); }
                    if g.font_name.is_none() { g.font_name = p_g.font_name.clone(); }
                    if g.font_size.is_none() { g.font_size = p_g.font_size; }
                    if g.font_weight.is_none() { g.font_weight = p_g.font_weight.clone(); }
                    if g.graphics_text.is_none() { g.graphics_text = p_g.graphics_text.clone(); }
                    if g.frame_title.is_none() { g.frame_title = p_g.frame_title.clone(); }
                    if g.frame_title_fore_color.is_none() { g.frame_title_fore_color = p_g.frame_title_fore_color.clone(); }
                    if g.frame_title_back_color.is_none() { g.frame_title_back_color = p_g.frame_title_back_color.clone(); }
                    if g.frame_title_font_name.is_none() { g.frame_title_font_name = p_g.frame_title_font_name.clone(); }
                    if g.frame_title_font_size.is_none() { g.frame_title_font_size = p_g.frame_title_font_size; }
                    if g.frame_title_font_weight.is_none() { g.frame_title_font_weight = p_g.frame_title_font_weight.clone(); }
                    if g.frame_title_visual_attribute.is_none() { g.frame_title_visual_attribute = p_g.frame_title_visual_attribute.clone(); }
                }
            }
        }
        if let Some(ref ft_va) = g.frame_title_visual_attribute {
            if let Some(va_style) = resolve_va_style(&mod_snapshot, &parents, ft_va, &mut HashSet::new()) {
                if g.frame_title_fore_color.is_none() { g.frame_title_fore_color = va_style.fore_color; }
                if g.frame_title_back_color.is_none() { g.frame_title_back_color = va_style.back_color; }
                if g.frame_title_font_name.is_none() { g.frame_title_font_name = va_style.font_name; }
                if g.frame_title_font_size.is_none() { g.frame_title_font_size = va_style.font_size; }
                if g.frame_title_font_weight.is_none() { g.frame_title_font_weight = va_style.font_weight; }
            }
        }
    }
}

fn strip_plsql_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' { out.push('\n'); }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn extract_first_quoted_arg(line_upper: &str, line_orig: &str, fn_upper: &str) -> Option<String> {
    let idx = line_upper.find(fn_upper)?;
    let rest = &line_orig[idx + fn_upper.len()..];
    let q1 = rest.find('\'')?;
    let after_q1 = &rest[q1 + 1..];
    let q2 = after_q1.find('\'')?;
    Some(after_q1[..q2].trim().to_string())
}

fn extract_branch_for_tab(src: &str, tab_name: &str) -> Option<String> {
    let clean = strip_plsql_comments(src);
    let needle = format!("'{}'", tab_name.to_ascii_uppercase());
    let lines: Vec<&str> = clean.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let up = line.to_ascii_uppercase();
        if (up.contains("IF ") || up.contains("ELSIF ") || up.contains("WHEN ")) && up.contains(&needle) {
            let mut body = String::new();
            let mut depth = 0i32;
            for &l in &lines[idx + 1..] {
                let lu = l.trim().to_ascii_uppercase();
                if depth == 0 && (lu.starts_with("ELSIF ") || lu.starts_with("ELSE") || lu.starts_with("END IF") || lu.starts_with("WHEN ")) {
                    break;
                }
                if lu.starts_with("IF ") || lu.contains(" THEN") && !lu.starts_with("ELSIF ") && !lu.starts_with("ELSE") {
                    if lu.starts_with("IF ") { depth += 1; }
                }
                if lu.starts_with("END IF") {
                    depth = (depth - 1).max(0);
                }
                body.push_str(l);
                body.push('\n');
            }
            if !body.trim().is_empty() {
                return Some(body);
            }
        }
    }
    None
}

fn collect_default_views(
    src: &str,
    canvas_lookup: &HashMap<String, String>,
    exclude_canvas: &str,
    pu_texts: &HashMap<String, String>,
    depth: usize,
) -> Vec<String> {
    let clean = strip_plsql_comments(src);
    let mut unconditional = Vec::new();
    let mut if_branch = Vec::new();
    let mut else_branch = Vec::new();
    let mut has_else = false;
    let mut branch_state = 0u8; // 0 = unconditional, 1 = if, 2 = elsif, 3 = else
    let mut if_depth = 0i32;

    for line in clean.lines() {
        let trimmed = line.trim();
        let up = trimmed.to_ascii_uppercase();
        if up.starts_with("IF ") {
            if if_depth == 0 {
                branch_state = 1;
            }
            if_depth += 1;
        } else if up.starts_with("ELSIF ") && if_depth == 1 {
            branch_state = 2;
        } else if (up == "ELSE" || up.starts_with("ELSE ")) && if_depth == 1 {
            branch_state = 3;
            has_else = true;
        } else if up.starts_with("END IF") {
            if_depth = (if_depth - 1).max(0);
            if if_depth == 0 {
                branch_state = 0;
            }
        }

        let mut found: Vec<String> = Vec::new();
        for fn_pat in ["SHOW_VIEW(", "SHOW_VIEW (", "GET_CANVAS_PROPERTY(", "SET_CANVAS_PROPERTY("] {
            if let Some(arg) = extract_first_quoted_arg(&up, trimmed, fn_pat) {
                if !fn_pat.starts_with("SHOW_VIEW") && !up.contains("TOPMOST_TAB_PAGE") {
                    continue;
                }
                if let Some(real_name) = canvas_lookup.get(&arg.to_ascii_uppercase()) {
                    if !real_name.eq_ignore_ascii_case(exclude_canvas) {
                        found.push(real_name.clone());
                    }
                }
            }
        }

        if depth < 2 && (branch_state == 0 || branch_state == 1 || branch_state == 3) {
            for (pu_up, pu_src) in pu_texts {
                if up.contains(pu_up) && !up.starts_with("PROCEDURE ") && !up.starts_with("FUNCTION ") {
                    let sub = collect_default_views(pu_src, canvas_lookup, exclude_canvas, pu_texts, depth + 1);
                    found.extend(sub);
                }
            }
        }

        for name in found {
            match branch_state {
                0 => if !unconditional.contains(&name) { unconditional.push(name); },
                1 => if !if_branch.contains(&name) { if_branch.push(name); },
                3 => if !else_branch.contains(&name) { else_branch.push(name); },
                _ => {}
            }
        }
    }

    let mut result = unconditional;
    let chosen = if has_else { else_branch } else { if_branch };
    for name in chosen {
        if !result.contains(&name) {
            result.push(name);
        }
    }
    result
}

fn resolve_tab_page_associations(index: &mut ModuleIndex, cache_dir: &Path) {
    let canvas_lookup: HashMap<String, String> = index
        .canvases
        .iter()
        .map(|c| (c.name.to_ascii_uppercase(), c.name.clone()))
        .collect();

    let mut pu_texts: HashMap<String, String> = HashMap::new();
    for pu in &index.program_units {
        if let Some(ref sref) = pu.text_ref {
            if let Ok(txt) = fs::read_to_string(cache_dir.join(&sref.file)) {
                pu_texts.insert(pu.name.to_ascii_uppercase(), txt);
            }
        }
    }

    let mut tab_triggers: Vec<String> = Vec::new();
    for tr in &index.triggers {
        if tr.name.eq_ignore_ascii_case("WHEN-TAB-PAGE-CHANGED") {
            if let Some(ref sref) = tr.text_ref {
                if let Ok(txt) = fs::read_to_string(cache_dir.join(&sref.file)) {
                    tab_triggers.push(txt);
                }
            }
        }
    }

    // Collect direct item/graphic counts per (canvas_upper, tab_page_upper)
    let mut direct_counts: HashMap<(String, String), usize> = HashMap::new();
    for b in &index.blocks {
        for it in &b.items {
            if let (Some(cn), Some(tp)) = (&it.canvas_name, &it.tab_page_name) {
                *direct_counts.entry((cn.to_ascii_uppercase(), tp.to_ascii_uppercase())).or_default() += 1;
            }
        }
    }
    for g in &index.graphics {
        if let (Some(cn), Some(tp)) = (&g.canvas_name, &g.tab_page_name) {
            *direct_counts.entry((cn.to_ascii_uppercase(), tp.to_ascii_uppercase())).or_default() += 1;
        }
    }

    let canvas_snapshot = index.canvases.clone();
    for canvas in &mut index.canvases {
        if canvas.tab_pages.is_empty() {
            continue;
        }
        let c_up = canvas.name.to_ascii_uppercase();
        for tp in &mut canvas.tab_pages {
            let mut assoc: Vec<String> = Vec::new();
            // 1. Check ProgramUnits and WHEN-TAB-PAGE-CHANGED for a branch matching tp.name
            for pu_src in pu_texts.values() {
                if let Some(branch) = extract_branch_for_tab(pu_src, &tp.name) {
                    for name in collect_default_views(&branch, &canvas_lookup, &canvas.name, &pu_texts, 0) {
                        if !assoc.contains(&name) {
                            assoc.push(name);
                        }
                    }
                }
            }
            if assoc.is_empty() {
                for tr_src in &tab_triggers {
                    if let Some(branch) = extract_branch_for_tab(tr_src, &tp.name) {
                        for name in collect_default_views(&branch, &canvas_lookup, &canvas.name, &pu_texts, 0) {
                            if !assoc.contains(&name) {
                                assoc.push(name);
                            }
                        }
                    }
                }
            }

            // 2. Fallback for empty TabPages: match overlay canvases in same window by TabPage stem
            let count = direct_counts.get(&(c_up.clone(), tp.name.to_ascii_uppercase())).copied().unwrap_or(0);
            if assoc.is_empty() && count == 0 {
                let tp_up = tp.name.to_ascii_uppercase();
                let stem = tp_up
                    .strip_suffix("_TAB")
                    .or_else(|| tp_up.strip_prefix("TAB_"))
                    .unwrap_or(&tp_up)
                    .trim_end_matches('S');
                if stem.len() >= 4 {
                    for other in &canvas_snapshot {
                        let is_overlay = matches!(other.canvas_type.as_deref(), Some("Tab") | Some("Stacked"))
                            || (other.canvas_type.is_none() && (other.viewport_x_position > 0 || other.viewport_y_position > 0));
                        if !other.name.eq_ignore_ascii_case(&canvas.name)
                            && other.window_name == canvas.window_name
                            && is_overlay
                            && other.name.to_ascii_uppercase().contains(stem)
                        {
                            assoc.push(other.name.clone());
                        }
                    }
                }
            }
            tp.associated_canvases = assoc;
        }
    }
}

#[cfg(test)]
mod disk_save_tests {
    use super::*;

    #[test]
    fn save_updates_single_file_in_place_without_numbered_copies() {
        let directory = std::env::temp_dir().join(format!("ofw-save-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let first = FormsService::write_form_file(&directory, "test", "<Module original=\"yes\"/>").unwrap();
        let second = FormsService::write_form_file(&directory, "test", "<Module edited=\"yes\"/>").unwrap();
        assert_eq!(first, "test_fmb.xml");
        assert_eq!(second, "test_fmb.xml");
        assert_eq!(fs::read_to_string(directory.join("test_fmb.xml")).unwrap(), "<Module edited=\"yes\"/>");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn non_sample_or_stale_samples_path_never_saves_into_samples() {
        let stale_path = FormsService::samples_dir().join("stale_non_sample_fmb.xml");
        assert!(!stale_path.exists());
        assert!(!FormsService::is_in_samples_dir(&stale_path));
        assert!(!FormsService::is_in_samples_dir(Path::new("/mnt/c/external/custom.fmb")));

        let extra_dir = std::env::temp_dir().join(format!("ofw-extra-{}", std::process::id()));
        fs::create_dir_all(&extra_dir).unwrap();
        let (dir_with_extra, prefix_with_extra) = FormsService::resolve_save_destination(
            &stale_path.to_string_lossy(),
            std::slice::from_ref(&extra_dir),
        );
        assert_eq!(dir_with_extra, extra_dir);
        assert_eq!(prefix_with_extra, extra_dir.to_string_lossy());

        let (dir_without_extra, prefix_without_extra) =
            FormsService::resolve_save_destination(&stale_path.to_string_lossy(), &[]);
        assert_eq!(dir_without_extra, FormsService::temp_saved_dir());
        assert_eq!(prefix_without_extra, FormsService::temp_saved_dir().to_string_lossy());

        let cache_dir = std::env::temp_dir().join(format!("ofw-save-cache-{}", std::process::id()));
        let service = FormsService::new(&cache_dir);
        let mut module = service.load_sample("orders_fmb.xml").unwrap();
        module.key.name = "EXT_TEMP_TEST".to_string();
        module.source_file = stale_path.to_string_lossy().to_string();
        service.save_module(module);

        let (saved_path, file_name, _, _) = service.save_form_file("EXT_TEMP_TEST").unwrap();
        assert_eq!(file_name, "ext_temp_test_fmb.xml");
        assert!(!stale_path.exists());
        assert!(!FormsService::is_in_samples_dir(&saved_path));
        assert!(saved_path.is_file());
        let _ = fs::remove_file(&saved_path);
        let _ = fs::remove_dir_all(&extra_dir);
        let _ = fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn resolves_visual_attribute_property_class_and_sibling_olb_fmb_inheritance() {
        let dir = std::env::temp_dir().join(format!("ofw-inherit-{}", std::process::id()));
        let cache = dir.join("cache");
        fs::create_dir_all(&dir).unwrap();

        let olb_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Module version="14.1.2.0.0" xmlns="http://xmlns.oracle.com/Forms">
  <ObjectLibrary Name="UTIL_STD">
    <ObjectLibraryTab Name="TAB_UI">
      <VisualAttribute Name="VA_INPUT" BackColor="r100g100b80" ForegroundColor="darkblue" FontName="Tahoma" FontSize="900"/>
      <PropertyClass Name="PC_STD_BTN" ItemType="Push Button" Width="110" Height="28" BackColor="gray12" Bevel="Raised" FontWeight="Bold"/>
    </ObjectLibraryTab>
  </ObjectLibrary>
</Module>"#;
        fs::write(dir.join("util_std_olb.xml"), olb_xml).unwrap();

        let parent_fmb_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Module version="14.1.2.0.0" xmlns="http://xmlns.oracle.com/Forms">
  <FormModule Name="BASE_FORM">
    <Block Name="EMP">
      <Item Name="EMPNO" ItemType="Text Item" Width="90" Height="20" Prompt="Employee No" Bevel="Lowered"/>
    </Block>
  </FormModule>
</Module>"#;
        fs::write(dir.join("base_form_fmb.xml"), parent_fmb_xml).unwrap();

        let child_fmb_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Module version="14.1.2.0.0" xmlns="http://xmlns.oracle.com/Forms">
  <FormModule Name="CHILD_EMP">
    <VisualAttribute Name="VA_LOCAL" BackColor="white" ParentFilename="util_std.olb" ParentName="VA_INPUT"/>
    <PropertyClass Name="PC_LOCAL_INPUT" VisualAttributeName="VA_LOCAL" Width="150"/>
    <Block Name="EMP">
      <Item Name="EMPNO" ParentModule="BASE_FORM" ParentFilename="base_form.fmb" ParentName="EMPNO" VisualAttributeName="VA_LOCAL" CanvasName="MAIN"/>
      <Item Name="ENAME" ParentName="PC_LOCAL_INPUT" CanvasName="MAIN" Prompt="Name"/>
      <Item Name="BTN_OK" ParentFilename="util_std.olb" ParentName="PC_STD_BTN" CanvasName="MAIN" Label="OK"/>
    </Block>
    <Canvas Name="MAIN" CanvasType="Content" Width="500" Height="300"/>
  </FormModule>
</Module>"#;
        let child_path = dir.join("child_emp_fmb.xml");
        fs::write(&child_path, child_fmb_xml).unwrap();

        let service = FormsService::new(&cache);
        let idx = service.load_module(&child_path).unwrap();
        let emp = &idx.blocks[0];

        // EMPNO inherits Width/Height/Prompt/Bevel from base_form_fmb.xml + colors/font from VA_LOCAL -> util_std_olb.xml
        assert_eq!(emp.items[0].width, Some(90));
        assert_eq!(emp.items[0].height, Some(20));
        assert_eq!(emp.items[0].prompt.as_deref(), Some("Employee No"));
        assert_eq!(emp.items[0].bevel.as_deref(), Some("Lowered"));
        assert_eq!(emp.items[0].back_color.as_deref(), Some("white")); // local VA_LOCAL overrides parent VA_INPUT
        assert_eq!(emp.items[0].fore_color.as_deref(), Some("darkblue")); // inherited from VA_INPUT in OLB
        assert_eq!(emp.items[0].font_name.as_deref(), Some("Tahoma"));
        assert_eq!(emp.items[0].font_size, Some(900));

        // ENAME inherits from local PC_LOCAL_INPUT -> VA_LOCAL -> util_std_olb.xml
        assert_eq!(emp.items[1].width, Some(150));
        assert_eq!(emp.items[1].fore_color.as_deref(), Some("darkblue"));

        // BTN_OK inherits from PC_STD_BTN in util_std_olb.xml
        assert_eq!(emp.items[2].item_type.as_deref(), Some("Push Button"));
        assert_eq!(emp.items[2].width, Some(110));
        assert_eq!(emp.items[2].height, Some(28));
        assert_eq!(emp.items[2].back_color.as_deref(), Some("gray12"));
        assert_eq!(emp.items[2].bevel.as_deref(), Some("Raised"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolves_tab_page_associated_stacked_and_nested_tab_canvases() {
        let dir = std::env::temp_dir().join(format!("ofw-tab-assoc-{}", std::process::id()));
        let cache = dir.join("cache");
        fs::create_dir_all(&dir).unwrap();

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Module version="14.1.2.0.0" xmlns="http://xmlns.oracle.com/Forms">
  <FormModule Name="DEPT_MOD">
    <Trigger Name="WHEN-TAB-PAGE-CHANGED" TriggerText="BEGIN&#10;  TAB_CLICK('CV_TAB_MAIN', :SYSTEM.TAB_NEW_PAGE);&#10;END;"/>
    <ProgramUnit Name="GO_SEARCH_TAB" ProgramUnitType="Procedure" ProgramUnitText="PROCEDURE GO_SEARCH_TAB IS&#10;BEGIN&#10;  SHOW_VIEW('CV_SEARCH_MAIN');&#10;  IF :BLK.MODE = 'ALT' THEN&#10;    SHOW_VIEW('CV_SEARCH_ALT');&#10;  ELSE&#10;    SHOW_VIEW('CV_SEARCH_DEFAULT');&#10;  END IF;&#10;END;"/>
    <ProgramUnit Name="TAB_CLICK" ProgramUnitType="Procedure" ProgramUnitText="PROCEDURE TAB_CLICK(p_cv IN VARCHAR2, p_tab IN VARCHAR2) IS&#10;BEGIN&#10;  IF p_tab = 'SEARCH_TAB' THEN&#10;    GO_SEARCH_TAB;&#10;  ELSIF p_tab = 'RESULTS_TAB' THEN&#10;    SET_CANVAS_PROPERTY('CV_NESTED_TABS', TOPMOST_TAB_PAGE, 'SUB_A');&#10;  END IF;&#10;END;"/>
    <Canvas Name="CV_TAB_MAIN" CanvasType="Tab" WindowName="WIN0" Width="800" Height="500">
      <TabPage Name="SEARCH_TAB" Label="Search"/>
      <TabPage Name="SUMMARY_TAB" Label="Summary"/>
      <TabPage Name="RESULTS_TAB" Label="Results"/>
    </Canvas>
    <Canvas Name="CV_SEARCH_MAIN" WindowName="WIN0" ViewportXPosition="10" ViewportYPosition="40" Width="700" Height="150"/>
    <Canvas Name="CV_SEARCH_ALT" WindowName="WIN0" ViewportXPosition="10" ViewportYPosition="200" Width="700" Height="150"/>
    <Canvas Name="CV_SEARCH_DEFAULT" WindowName="WIN0" ViewportXPosition="10" ViewportYPosition="200" Width="700" Height="150"/>
    <Canvas Name="CV_NESTED_TABS" CanvasType="Tab" WindowName="WIN0" ViewportXPosition="10" ViewportYPosition="40" Width="700" Height="400">
      <TabPage Name="SUB_A" Label="Sub A"/>
    </Canvas>
  </FormModule>
</Module>"#;
        let fpath = dir.join("dept_tab_fmb.xml");
        fs::write(&fpath, xml).unwrap();

        let service = FormsService::new(&cache);
        let idx = service.load_module(&fpath).unwrap();
        let main_tab = &idx.canvases[0];
        assert_eq!(main_tab.tab_pages[0].associated_canvases, vec!["CV_SEARCH_MAIN", "CV_SEARCH_DEFAULT"]);
        assert!(main_tab.tab_pages[1].associated_canvases.is_empty());
        assert_eq!(main_tab.tab_pages[2].associated_canvases, vec!["CV_NESTED_TABS"]);

        let _ = fs::remove_dir_all(&dir);
    }
}
