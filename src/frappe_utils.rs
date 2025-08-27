use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrappeApp {
    pub name: String,
    pub path: PathBuf,
    pub module_path: PathBuf,
    pub hooks_path: PathBuf,
    pub doctypes: Vec<DocTypeInfo>,
    pub pages: Vec<PageInfo>,
    pub reports: Vec<ReportInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocTypeInfo {
    pub name: String,
    pub module: String,
    pub file_path: PathBuf,
    pub controller_path: Option<PathBuf>,
    pub client_script_path: Option<PathBuf>,
    pub fields: Vec<FieldInfo>,
    pub permissions: Vec<PermissionInfo>,
    pub links: Vec<LinkInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub fieldname: String,
    pub fieldtype: String,
    pub label: String,
    pub options: Option<String>,
    pub reqd: Option<i32>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionInfo {
    pub role: String,
    pub read: Option<i32>,
    pub write: Option<i32>,
    pub create: Option<i32>,
    pub delete: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkInfo {
    pub source_field: String,
    pub target_doctype: String,
    pub link_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub name: String,
    pub title: String,
    pub module: String,
    pub route: String,
    pub file_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportInfo {
    pub name: String,
    pub report_type: String,
    pub module: String,
    pub file_path: PathBuf,
    pub query_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FrappeProject {
    pub bench_path: PathBuf,
    pub apps: Vec<FrappeApp>,
    pub sites: Vec<SiteInfo>,
    pub default_site: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteInfo {
    pub name: String,
    pub path: PathBuf,
    pub config_path: PathBuf,
    pub database: Option<String>,
}

pub struct FrappeAnalyzer {
    project: Option<FrappeProject>,
}

impl FrappeAnalyzer {
    pub fn new() -> Self {
        Self { project: None }
    }

    pub fn analyze_project(&mut self, workspace_path: &Path) -> Result<(), String> {
        if !self.is_frappe_workspace(workspace_path) {
            return Err("Not a valid Frappe workspace".to_string());
        }

        let bench_path = workspace_path.to_path_buf();
        let apps = self.discover_apps(&bench_path)?;
        let sites = self.discover_sites(&bench_path)?;
        let default_site = self.get_default_site(&bench_path)?;

        self.project = Some(FrappeProject {
            bench_path,
            apps,
            sites,
            default_site,
        });

        Ok(())
    }

    pub fn is_frappe_workspace(&self, path: &Path) -> bool {
        let apps_txt = path.join("apps.txt");
        let sites_dir = path.join("sites");
        let procfile = path.join("Procfile");

        apps_txt.exists()
            && sites_dir.exists()
            && (procfile.exists() || path.join("bench-repo").exists())
    }

    pub fn discover_apps(&self, bench_path: &Path) -> Result<Vec<FrappeApp>, String> {
        let apps_txt_path = bench_path.join("apps.txt");
        let apps_content =
            fs::read_to_string(apps_txt_path).map_err(|_| "Could not read apps.txt".to_string())?;

        let mut apps = Vec::new();
        let apps_dir = bench_path.join("apps");

        for line in apps_content.lines() {
            let app_name = line.trim();
            if app_name.is_empty() || app_name.starts_with('#') {
                continue;
            }

            let app_path = apps_dir.join(app_name);
            if app_path.exists() {
                if let Ok(app) = self.analyze_app(app_name, &app_path) {
                    apps.push(app);
                }
            }
        }

        Ok(apps)
    }

    pub fn analyze_app(&self, name: &str, path: &Path) -> Result<FrappeApp, String> {
        let module_path = path.join(name);
        let hooks_path = path.join(name).join("hooks.py");

        let doctypes = self.discover_doctypes(&module_path)?;
        let pages = self.discover_pages(&module_path)?;
        let reports = self.discover_reports(&module_path)?;

        Ok(FrappeApp {
            name: name.to_string(),
            path: path.to_path_buf(),
            module_path,
            hooks_path,
            doctypes,
            pages,
            reports,
        })
    }

    pub fn discover_doctypes(&self, module_path: &Path) -> Result<Vec<DocTypeInfo>, String> {
        let mut doctypes = Vec::new();

        for entry in fs::read_dir(module_path).map_err(|_| "Could not read module directory")? {
            let entry = entry.map_err(|_| "Invalid directory entry")?;
            let path = entry.path();

            if path.is_dir() {
                let doctype_dir = path.join("doctype");
                if doctype_dir.exists() {
                    if let Ok(dt_doctypes) = self.scan_doctype_directory(&doctype_dir) {
                        doctypes.extend(dt_doctypes);
                    }
                }
            }
        }

        Ok(doctypes)
    }

    pub fn scan_doctype_directory(&self, doctype_dir: &Path) -> Result<Vec<DocTypeInfo>, String> {
        let mut doctypes = Vec::new();

        for entry in fs::read_dir(doctype_dir).map_err(|_| "Could not read doctype directory")? {
            let entry = entry.map_err(|_| "Invalid directory entry")?;
            let path = entry.path();

            if path.is_dir() {
                let doctype_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                if let Ok(doctype_info) = self.parse_doctype(&path, &doctype_name) {
                    doctypes.push(doctype_info);
                }
            }
        }

        Ok(doctypes)
    }

    pub fn parse_doctype(&self, doctype_path: &Path, name: &str) -> Result<DocTypeInfo, String> {
        let json_file =
            doctype_path.join(format!("{}.json", name.to_lowercase().replace(" ", "_")));

        if !json_file.exists() {
            return Err(format!("DocType JSON not found: {}", json_file.display()));
        }

        let content = fs::read_to_string(&json_file).map_err(|_| "Could not read DocType JSON")?;

        let json_value: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| "Invalid JSON format")?;

        let module = json_value
            .get("module")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let fields = self.parse_fields(&json_value)?;
        let permissions = self.parse_permissions(&json_value)?;
        let links = self.analyze_doctype_links(&fields);

        let controller_name = name.to_lowercase().replace(" ", "_");
        let controller_path = doctype_path.join(format!("{}.py", controller_name));
        let client_script_path = doctype_path.join(format!("{}.js", controller_name));

        Ok(DocTypeInfo {
            name: name.to_string(),
            module,
            file_path: json_file,
            controller_path: if controller_path.exists() {
                Some(controller_path)
            } else {
                None
            },
            client_script_path: if client_script_path.exists() {
                Some(client_script_path)
            } else {
                None
            },
            fields,
            permissions,
            links,
        })
    }

    pub fn parse_fields(&self, json_value: &serde_json::Value) -> Result<Vec<FieldInfo>, String> {
        let fields_array = json_value
            .get("fields")
            .and_then(|v| v.as_array())
            .ok_or("No fields array found")?;

        let mut fields = Vec::new();
        for field_val in fields_array {
            if let Ok(field) = self.parse_single_field(field_val) {
                fields.push(field);
            }
        }

        Ok(fields)
    }

    pub fn parse_single_field(&self, field_val: &serde_json::Value) -> Result<FieldInfo, String> {
        let fieldname = field_val
            .get("fieldname")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let fieldtype = field_val
            .get("fieldtype")
            .and_then(|v| v.as_str())
            .unwrap_or("Data")
            .to_string();

        let label = field_val
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or(&fieldname)
            .to_string();

        let options = field_val
            .get("options")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let reqd = field_val
            .get("reqd")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32);

        let description = field_val
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(FieldInfo {
            fieldname,
            fieldtype,
            label,
            options,
            reqd,
            description,
        })
    }

    pub fn parse_permissions(
        &self,
        json_value: &serde_json::Value,
    ) -> Result<Vec<PermissionInfo>, String> {
        let perms_array = json_value
            .get("permissions")
            .and_then(|v| v.as_array())
            .cloned() // clone the Vec<Value>
            .unwrap_or_default(); // empty Vec if none;

        let mut permissions = Vec::new();
        for perm_val in perms_array {
            if let Ok(perm) = self.parse_single_permission(&perm_val) {
                permissions.push(perm);
            }
        }

        Ok(permissions)
    }

    pub fn parse_single_permission(
        &self,
        perm_val: &serde_json::Value,
    ) -> Result<PermissionInfo, String> {
        let role = perm_val
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let read = perm_val
            .get("read")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32);
        let write = perm_val
            .get("write")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32);
        let create = perm_val
            .get("create")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32);
        let delete = perm_val
            .get("delete")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32);

        Ok(PermissionInfo {
            role,
            read,
            write,
            create,
            delete,
        })
    }

    pub fn analyze_doctype_links(&self, fields: &[FieldInfo]) -> Vec<LinkInfo> {
        let mut links = Vec::new();

        for field in fields {
            match field.fieldtype.as_str() {
                "Link" => {
                    if let Some(target) = &field.options {
                        links.push(LinkInfo {
                            source_field: field.fieldname.clone(),
                            target_doctype: target.clone(),
                            link_type: "Link".to_string(),
                        });
                    }
                }
                "Dynamic Link" => {
                    links.push(LinkInfo {
                        source_field: field.fieldname.clone(),
                        target_doctype: "Dynamic".to_string(),
                        link_type: "Dynamic Link".to_string(),
                    });
                }
                "Table" => {
                    if let Some(target) = &field.options {
                        links.push(LinkInfo {
                            source_field: field.fieldname.clone(),
                            target_doctype: target.clone(),
                            link_type: "Table".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        links
    }

    pub fn discover_pages(&self, module_path: &Path) -> Result<Vec<PageInfo>, String> {
        let mut pages = Vec::new();

        for entry in fs::read_dir(module_path).map_err(|_| "Could not read module directory")? {
            let entry = entry.map_err(|_| "Invalid directory entry")?;
            let path = entry.path();

            if path.is_dir() {
                let page_dir = path.join("page");
                if page_dir.exists() {
                    if let Ok(module_pages) = self.scan_page_directory(&page_dir) {
                        pages.extend(module_pages);
                    }
                }
            }
        }

        Ok(pages)
    }

    pub fn scan_page_directory(&self, page_dir: &Path) -> Result<Vec<PageInfo>, String> {
        let mut pages = Vec::new();

        for entry in fs::read_dir(page_dir).map_err(|_| "Could not read page directory")? {
            let entry = entry.map_err(|_| "Invalid directory entry")?;
            let path = entry.path();

            if path.is_dir() {
                let page_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                if let Ok(page_info) = self.parse_page(&path, &page_name) {
                    pages.push(page_info);
                }
            }
        }

        Ok(pages)
    }

    pub fn parse_page(&self, page_path: &Path, name: &str) -> Result<PageInfo, String> {
        let json_file = page_path.join(format!("{}.json", name));

        if !json_file.exists() {
            return Err(format!("Page JSON not found: {}", json_file.display()));
        }

        let content = fs::read_to_string(&json_file).map_err(|_| "Could not read Page JSON")?;

        let json_value: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| "Invalid JSON format")?;

        let title = json_value
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .to_string();

        let module = json_value
            .get("module")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let route = json_value
            .get("route")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .to_string();

        Ok(PageInfo {
            name: name.to_string(),
            title,
            module,
            route,
            file_path: json_file,
        })
    }

    pub fn discover_reports(&self, module_path: &Path) -> Result<Vec<ReportInfo>, String> {
        let mut reports = Vec::new();

        for entry in fs::read_dir(module_path).map_err(|_| "Could not read module directory")? {
            let entry = entry.map_err(|_| "Invalid directory entry")?;
            let path = entry.path();

            if path.is_dir() {
                let report_dir = path.join("report");
                if report_dir.exists() {
                    if let Ok(module_reports) = self.scan_report_directory(&report_dir) {
                        reports.extend(module_reports);
                    }
                }
            }
        }

        Ok(reports)
    }

    pub fn scan_report_directory(&self, report_dir: &Path) -> Result<Vec<ReportInfo>, String> {
        let mut reports = Vec::new();

        for entry in fs::read_dir(report_dir).map_err(|_| "Could not read report directory")? {
            let entry = entry.map_err(|_| "Invalid directory entry")?;
            let path = entry.path();

            if path.is_dir() {
                let report_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                if let Ok(report_info) = self.parse_report(&path, &report_name) {
                    reports.push(report_info);
                }
            }
        }

        Ok(reports)
    }

    pub fn parse_report(&self, report_path: &Path, name: &str) -> Result<ReportInfo, String> {
        let json_file = report_path.join(format!("{}.json", name.to_lowercase().replace(" ", "_")));

        if !json_file.exists() {
            return Err(format!("Report JSON not found: {}", json_file.display()));
        }

        let content = fs::read_to_string(&json_file).map_err(|_| "Could not read Report JSON")?;

        let json_value: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| "Invalid JSON format")?;

        let report_type = json_value
            .get("report_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Report Builder")
            .to_string();

        let module = json_value
            .get("module")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let query_type = json_value
            .get("query_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(ReportInfo {
            name: name.to_string(),
            report_type,
            module,
            file_path: json_file,
            query_type,
        })
    }

    pub fn discover_sites(&self, bench_path: &Path) -> Result<Vec<SiteInfo>, String> {
        let sites_dir = bench_path.join("sites");
        let mut sites = Vec::new();

        if !sites_dir.exists() {
            return Ok(sites);
        }

        for entry in fs::read_dir(&sites_dir).map_err(|_| "Could not read sites directory")? {
            let entry = entry.map_err(|_| "Invalid directory entry")?;
            let path = entry.path();

            if path.is_dir() {
                let site_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                // Skip common directories
                if site_name == "assets" || site_name.starts_with('.') {
                    continue;
                }

                let config_path = path.join("site_config.json");
                let database = self.extract_database_name(&config_path).ok();

                sites.push(SiteInfo {
                    name: site_name,
                    path,
                    config_path,
                    database,
                });
            }
        }

        Ok(sites)
    }

    pub fn extract_database_name(&self, config_path: &Path) -> Result<String, String> {
        if !config_path.exists() {
            return Err("Site config not found".to_string());
        }

        let content = fs::read_to_string(config_path).map_err(|_| "Could not read site config")?;

        let config: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| "Invalid site config JSON")?;

        let db_name = config
            .get("db_name")
            .and_then(|v| v.as_str())
            .ok_or("Database name not found in config")?;

        Ok(db_name.to_string())
    }

    pub fn get_default_site(&self, bench_path: &Path) -> Result<Option<String>, String> {
        let common_config_path = bench_path.join("sites").join("common_site_config.json");

        if !common_config_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(common_config_path)
            .map_err(|_| "Could not read common site config")?;

        let config: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| "Invalid common site config JSON")?;

        Ok(config
            .get("default_site")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    pub fn suggest_field_type(&self, field_name: &str) -> String {
        let name_lower = field_name.to_lowercase();

        // Email patterns
        if name_lower.contains("email") {
            return "Data".to_string(); // Could be enhanced to Email type
        }

        // Phone patterns
        if name_lower.contains("phone") || name_lower.contains("mobile") {
            return "Phone".to_string();
        }

        // Date patterns
        if name_lower.contains("date")
            || name_lower.ends_with("_on")
            || name_lower.contains("birth")
            || name_lower.contains("expiry")
        {
            return "Date".to_string();
        }

        // DateTime patterns
        if name_lower.contains("time")
            || name_lower.contains("created")
            || name_lower.contains("modified")
            || name_lower.contains("timestamp")
        {
            return "Datetime".to_string();
        }

        // Currency patterns
        if name_lower.contains("amount")
            || name_lower.contains("price")
            || name_lower.contains("cost")
            || name_lower.contains("rate")
        {
            return "Currency".to_string();
        }

        // Float patterns
        if name_lower.contains("percentage")
            || name_lower.contains("ratio")
            || name_lower.contains("weight")
            || name_lower.contains("qty")
        {
            return "Float".to_string();
        }

        // Int patterns
        if name_lower.contains("count")
            || name_lower.contains("number")
            || name_lower.contains("id") && !name_lower.contains("_id")
        {
            return "Int".to_string();
        }

        // Text patterns
        if name_lower.contains("description")
            || name_lower.contains("comment")
            || name_lower.contains("note")
            || name_lower.contains("remark")
        {
            return "Text".to_string();
        }

        // Link patterns
        if name_lower.ends_with("_id") || name_lower.contains("reference") {
            return "Link".to_string();
        }

        // Check patterns
        if name_lower.starts_with("is_")
            || name_lower.starts_with("has_")
            || name_lower.contains("enabled")
            || name_lower.contains("disabled")
        {
            return "Check".to_string();
        }

        // Default to Data
        "Data".to_string()
    }

    pub fn get_project(&self) -> Option<&FrappeProject> {
        self.project.as_ref()
    }

    pub fn search_doctypes(&self, query: &str) -> Vec<&DocTypeInfo> {
        if let Some(project) = &self.project {
            let mut results = Vec::new();
            let query_lower = query.to_lowercase();

            for app in &project.apps {
                for doctype in &app.doctypes {
                    if doctype.name.to_lowercase().contains(&query_lower)
                        || doctype.module.to_lowercase().contains(&query_lower)
                    {
                        results.push(doctype);
                    }
                }
            }

            results
        } else {
            Vec::new()
        }
    }

    pub fn find_doctype_dependencies(&self, doctype_name: &str) -> HashMap<String, Vec<String>> {
        let mut dependencies = HashMap::new();

        if let Some(project) = &self.project {
            for app in &project.apps {
                for dt in &app.doctypes {
                    if dt.name == doctype_name {
                        let mut deps = Vec::new();
                        for link in &dt.links {
                            if link.target_doctype != "Dynamic" {
                                deps.push(link.target_doctype.clone());
                            }
                        }
                        if !deps.is_empty() {
                            dependencies.insert("dependencies".to_string(), deps);
                        }
                    }

                    // Find reverse dependencies
                    for link in &dt.links {
                        if link.target_doctype == doctype_name {
                            dependencies
                                .entry("dependents".to_string())
                                .or_insert_with(Vec::new)
                                .push(dt.name.clone());
                        }
                    }
                }
            }
        }

        dependencies
    }
}

pub fn generate_field_suggestions(field_name: &str) -> Vec<(String, String)> {
    let analyzer = FrappeAnalyzer::new();
    let suggested_type = analyzer.suggest_field_type(field_name);

    let mut suggestions = vec![(
        suggested_type.clone(),
        "Primary suggestion based on field name".to_string(),
    )];

    // Add common alternatives
    match suggested_type.as_str() {
        "Data" => {
            suggestions.push((
                "Small Text".to_string(),
                "For longer text content".to_string(),
            ));
            suggestions.push(("Text".to_string(), "For very long text".to_string()));
        }
        "Link" => {
            suggestions.push((
                "Dynamic Link".to_string(),
                "For variable DocType links".to_string(),
            ));
            suggestions.push(("Data".to_string(), "If not linking to DocType".to_string()));
        }
        "Float" => {
            suggestions.push(("Currency".to_string(), "If represents money".to_string()));
            suggestions.push(("Int".to_string(), "If whole numbers only".to_string()));
        }
        _ => {}
    }

    suggestions
}
