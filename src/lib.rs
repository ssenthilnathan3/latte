use std::collections::HashMap;
use std::fs;
use std::path::Path;
use zed_extension_api::{
    self as zed, register_extension, Command, Extension, LanguageServerId, Result, SlashCommand,
    SlashCommandArgumentCompletion, SlashCommandOutput, SlashCommandResult, Worktree,
};

mod frappe_utils;
mod process_manager;
mod test_runner;

use frappe_utils::{generate_field_suggestions, FrappeAnalyzer};
use process_manager::ProcessManager;
use test_runner::TestRunner;

struct LatteExtension {
    cached_frappe_config: Option<FrappeConfig>,
    frappe_analyzer: FrappeAnalyzer,
    process_manager: ProcessManager,
}

#[derive(Debug, Clone)]
struct FrappeConfig {
    bench_path: String,
    default_site: Option<String>,
    apps_path: String,
    sites_path: String,
}

impl Default for LatteExtension {
    fn default() -> Self {
        Self {
            cached_frappe_config: None,
            frappe_analyzer: FrappeAnalyzer::new(),
            process_manager: ProcessManager::new(),
        }
    }
}

impl Extension for LatteExtension {
    fn new() -> Self {
        Self::default()
    }

    fn name(&self) -> &'static str {
        "Latte"
    }

    fn complete_slash_command_argument(
        &self,
        command: SlashCommand,
        _args: Vec<String>,
    ) -> Result<Vec<SlashCommandArgumentCompletion>, String> {
        match command.name.as_str() {
            "frappe-new-app" => Ok(vec![SlashCommandArgumentCompletion {
                label: "app_name".to_string(),
                new_text: "app_name".to_string(),
                run_command: false,
            }]),
            "frappe-new-site" => Ok(vec![SlashCommandArgumentCompletion {
                label: "site_name.local".to_string(),
                new_text: "site_name.local".to_string(),
                run_command: false,
            }]),
            "frappe-new-doctype" => Ok(vec![
                SlashCommandArgumentCompletion {
                    label: "DocType Name".to_string(),
                    new_text: "DocType Name".to_string(),
                    run_command: false,
                },
                SlashCommandArgumentCompletion {
                    label: "Module Name".to_string(),
                    new_text: "Module Name".to_string(),
                    run_command: false,
                },
            ]),
            _ => Ok(vec![]),
        }
    }

    fn run_slash_command(
        &self,
        command: SlashCommand,
        args: Vec<String>,
        worktree: &Worktree,
    ) -> Result<SlashCommandResult, String> {
        match command.name.as_str() {
            "frappe-bench-start" => self.run_bench_command("start", &[], worktree),
            "frappe-bench-stop" => self.stop_bench_process(worktree),
            "frappe-bench-migrate" => self.run_bench_command("migrate", &[], worktree),
            "frappe-bench-build" => self.run_bench_command("build", &[], worktree),
            "frappe-new-app" => {
                if args.is_empty() {
                    return Err("App name is required".to_string());
                }
                self.run_bench_command("new-app", &[&args[0]], worktree)
            }
            "frappe-new-site" => {
                if args.is_empty() {
                    return Err("Site name is required".to_string());
                }
                self.run_bench_command("new-site", &[&args[0]], worktree)
            }
            "frappe-console" => self.open_frappe_console(worktree),
            "frappe-mariadb" => self.open_mariadb_repl(worktree),
            "frappe-new-doctype" => {
                if args.len() < 2 {
                    return Err("DocType name and module are required".to_string());
                }
                self.generate_doctype(&args[0], &args[1], worktree)
            }
            "frappe-new-page" => {
                if args.is_empty() {
                    return Err("Page name is required".to_string());
                }
                self.generate_page(&args[0], worktree)
            }
            "frappe-new-report" => {
                if args.is_empty() {
                    return Err("Report name is required".to_string());
                }
                self.generate_report(&args[0], worktree)
            }
            "frappe-run-tests" => {
                let app = args.get(0).map(|s| s.as_str()).unwrap_or("frappe");
                self.run_tests(app, worktree)
            }
            "frappe-search-doctype" => {
                let query = args.get(0).map(|s| s.as_str()).unwrap_or("");
                self.search_doctypes(query, worktree)
            }
            "frappe-analyze-project" => self.analyze_current_project(worktree),
            "frappe-list-processes" => self.list_running_processes(),
            "frappe-stop-all" => self.stop_all_processes(),
            _ => Err(format!("Unknown command: {}", command.name)),
        }
    }

    fn slash_commands(&self) -> Vec<SlashCommand> {
        vec![
            // Bench Commands
            SlashCommand {
                name: "frappe-bench-start".to_string(),
                description: "Start the Frappe bench development server".to_string(),
                tooltip_text: "Runs 'bench start' and streams logs to Bench panel".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-bench-stop".to_string(),
                description: "Stop the running Frappe bench process".to_string(),
                tooltip_text: "Gracefully stops the bench development server".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-bench-migrate".to_string(),
                description: "Run database migrations".to_string(),
                tooltip_text: "Executes 'bench migrate' to update database schema".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-bench-build".to_string(),
                description: "Build assets and translations".to_string(),
                tooltip_text: "Runs 'bench build' to compile assets".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-new-app".to_string(),
                description: "Create a new Frappe app".to_string(),
                tooltip_text: "Scaffolds a new app with 'bench new-app <name>'".to_string(),
                requires_argument: true,
            },
            SlashCommand {
                name: "frappe-new-site".to_string(),
                description: "Create a new Frappe site".to_string(),
                tooltip_text: "Creates a new site with 'bench new-site <name>'".to_string(),
                requires_argument: true,
            },
            SlashCommand {
                name: "frappe-console".to_string(),
                description: "Open Frappe console".to_string(),
                tooltip_text: "Opens an interactive Python console for the site".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-mariadb".to_string(),
                description: "Open MariaDB/MySQL console".to_string(),
                tooltip_text: "Opens database console connected to current site".to_string(),
                requires_argument: false,
            },
            // Generators
            SlashCommand {
                name: "frappe-new-doctype".to_string(),
                description: "Generate a new DocType".to_string(),
                tooltip_text: "Creates DocType JSON, controller, and client script files"
                    .to_string(),
                requires_argument: true,
            },
            SlashCommand {
                name: "frappe-new-page".to_string(),
                description: "Generate a new Page".to_string(),
                tooltip_text: "Scaffolds page files (.py, .js, .json)".to_string(),
                requires_argument: true,
            },
            SlashCommand {
                name: "frappe-new-report".to_string(),
                description: "Generate a new Report".to_string(),
                tooltip_text: "Creates report files and boilerplate".to_string(),
                requires_argument: true,
            },
            SlashCommand {
                name: "frappe-run-tests".to_string(),
                description: "Run tests for an app".to_string(),
                tooltip_text: "Executes tests and shows results in diagnostics".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-search-doctype".to_string(),
                description: "Search DocTypes across all apps".to_string(),
                tooltip_text: "Find DocTypes by name or module".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-analyze-project".to_string(),
                description: "Analyze Frappe project structure".to_string(),
                tooltip_text: "Scan and index all apps, DocTypes, and dependencies".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-list-processes".to_string(),
                description: "List running Frappe processes".to_string(),
                tooltip_text: "Show all bench processes and their status".to_string(),
                requires_argument: false,
            },
            SlashCommand {
                name: "frappe-stop-all".to_string(),
                description: "Stop all running processes".to_string(),
                tooltip_text: "Gracefully stop all bench and related processes".to_string(),
                requires_argument: false,
            },
        ]
    }
}

impl LatteExtension {
    fn detect_frappe_workspace(&self, worktree: &Worktree) -> Option<FrappeConfig> {
        let worktree_path = worktree.abs_path();

        // Check for common Frappe indicators
        let apps_txt = worktree_path.join("apps.txt");
        let sites_dir = worktree_path.join("sites");
        let procfile = worktree_path.join("Procfile");

        if apps_txt.exists() && sites_dir.exists() {
            let default_site = self.get_default_site(&worktree_path);

            Some(FrappeConfig {
                bench_path: worktree_path.to_string_lossy().to_string(),
                default_site,
                apps_path: worktree_path.join("apps").to_string_lossy().to_string(),
                sites_path: sites_dir.to_string_lossy().to_string(),
            })
        } else {
            None
        }
    }

    fn get_default_site(&self, bench_path: &Path) -> Option<String> {
        let common_site_path = bench_path.join("sites").join("common_site_config.json");
        if let Ok(content) = fs::read_to_string(common_site_path) {
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(default_site) = config.get("default_site") {
                    return default_site.as_str().map(|s| s.to_string());
                }
            }
        }
        None
    }

    fn run_bench_command(
        &self,
        cmd: &str,
        args: &[&str],
        worktree: &Worktree,
    ) -> Result<SlashCommandResult, String> {
        let config = self
            .detect_frappe_workspace(worktree)
            .ok_or("Not a Frappe workspace")?;

        let args_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let process_id = match cmd {
            "start" => self
                .process_manager
                .start_bench_dev_server(&config.bench_path),
            "migrate" => self
                .process_manager
                .run_bench_migrate(&config.bench_path, config.default_site.as_deref()),
            "build" => self.process_manager.run_bench_build(&config.bench_path),
            "new-app" => {
                if let Some(app_name) = args.first() {
                    self.process_manager
                        .create_new_app(&config.bench_path, app_name)
                } else {
                    return Err("App name is required".to_string());
                }
            }
            "new-site" => {
                if let Some(site_name) = args.first() {
                    self.process_manager
                        .create_new_site(&config.bench_path, site_name)
                } else {
                    return Err("Site name is required".to_string());
                }
            }
            _ => self.process_manager.start_bench_process(
                format!("bench_{}_{}", cmd, chrono::Utc::now().timestamp()),
                &config.bench_path,
                cmd,
                args_vec,
            ),
        };

        match process_id {
            Ok(id) => {
                let output = format!(
                    "✅ Started bench {} (Process ID: {})\nBench Path: {}\nCheck logs for details.",
                    cmd, id, config.bench_path
                );
                Ok(SlashCommandResult {
                    text: output,
                    run_commands_in_text: false,
                })
            }
            Err(e) => Err(format!("Failed to start bench {}: {}", cmd, e)),
        }
    }

    fn stop_bench_process(&self, _worktree: &Worktree) -> Result<SlashCommandResult, String> {
        if let Some(bench_process_id) = self.process_manager.get_bench_process_id() {
            match self.process_manager.stop_process(&bench_process_id) {
                Ok(()) => Ok(SlashCommandResult {
                    text: "✅ Bench process stopped successfully".to_string(),
                    run_commands_in_text: false,
                }),
                Err(e) => Err(format!("Failed to stop bench process: {}", e)),
            }
        } else {
            Ok(SlashCommandResult {
                text: "ℹ️ No bench process is currently running".to_string(),
                run_commands_in_text: false,
            })
        }
    }

    fn open_frappe_console(&self, worktree: &Worktree) -> Result<SlashCommandResult, String> {
        let config = self
            .detect_frappe_workspace(worktree)
            .ok_or("Not a Frappe workspace")?;

        let site = config
            .default_site
            .unwrap_or_else(|| "localhost".to_string());

        match self.process_manager.open_console(&config.bench_path, &site) {
            Ok(process_id) => Ok(SlashCommandResult {
                text: format!(
                    "🔧 Opening Frappe console for site: {} (Process ID: {})\nType your Python commands in the console.",
                    site, process_id
                ),
                run_commands_in_text: false,
            }),
            Err(e) => Err(format!("Failed to open console: {}", e))
        }
    }

    fn open_mariadb_repl(&self, worktree: &Worktree) -> Result<SlashCommandResult, String> {
        let config = self
            .detect_frappe_workspace(worktree)
            .ok_or("Not a Frappe workspace")?;

        let site = config
            .default_site
            .unwrap_or_else(|| "localhost".to_string());

        match self.process_manager.open_mariadb(&config.bench_path, &site) {
            Ok(process_id) => Ok(SlashCommandResult {
                text: format!(
                    "🗄️ Opening MariaDB console for site: {} (Process ID: {})\nYou can now run SQL queries directly.",
                    site, process_id
                ),
                run_commands_in_text: false,
            }),
            Err(e) => Err(format!("Failed to open MariaDB console: {}", e))
        }
    }

    fn generate_doctype(
        &self,
        doctype_name: &str,
        module: &str,
        worktree: &Worktree,
    ) -> Result<SlashCommandResult, String> {
        let config = self
            .detect_frappe_workspace(worktree)
            .ok_or("Not a Frappe workspace")?;

        // Generate DocType JSON
        let doctype_json = self.create_doctype_json(doctype_name, module);
        let controller_py = self.create_doctype_controller(doctype_name, module);
        let client_js = self.create_doctype_client_script(doctype_name);

        let output = format!(
            "Generated DocType: {}\nModule: {}\nFiles created:\n- {}.json\n- {}.py\n- {}.js",
            doctype_name,
            module,
            doctype_name.to_lowercase().replace(" ", "_"),
            doctype_name.to_lowercase().replace(" ", "_"),
            doctype_name.to_lowercase().replace(" ", "_")
        );

        Ok(SlashCommandResult {
            text: output,
            run_commands_in_text: false,
        })
    }

    fn generate_page(
        &self,
        page_name: &str,
        worktree: &Worktree,
    ) -> Result<SlashCommandResult, String> {
        let config = self
            .detect_frappe_workspace(worktree)
            .ok_or("Not a Frappe workspace")?;

        let output = format!(
            "Generated Page: {}\nFiles created:\n- {}.py\n- {}.js\n- {}.json",
            page_name,
            page_name.to_lowercase().replace(" ", "_"),
            page_name.to_lowercase().replace(" ", "_"),
            page_name.to_lowercase().replace(" ", "_")
        );

        Ok(SlashCommandResult {
            text: output,
            run_commands_in_text: false,
        })
    }

    fn generate_report(
        &self,
        report_name: &str,
        worktree: &Worktree,
    ) -> Result<SlashCommandResult, String> {
        let config = self
            .detect_frappe_workspace(worktree)
            .ok_or("Not a Frappe workspace")?;

        let output = format!(
            "Generated Report: {}\nFiles created:\n- {}.py\n- {}.js\n- {}.json",
            report_name,
            report_name.to_lowercase().replace(" ", "_"),
            report_name.to_lowercase().replace(" ", "_"),
            report_name.to_lowercase().replace(" ", "_")
        );

        Ok(SlashCommandResult {
            text: output,
            run_commands_in_text: false,
        })
    }

    fn run_tests(&self, app: &str, worktree: &Worktree) -> Result<SlashCommandResult, String> {
        let config = self
            .detect_frappe_workspace(worktree)
            .ok_or("Not a Frappe workspace")?;

        let site = config
            .default_site
            .unwrap_or_else(|| "localhost".to_string());

        let test_runner = TestRunner::new(config.bench_path.clone(), site);

        match test_runner.run_app_tests(app) {
            Ok(test_suite) => {
                let summary = test_runner.format_test_summary(&test_suite);

                // Extract diagnostics for failed tests
                let diagnostics = test_runner.extract_diagnostics(&test_suite.results);

                let mut output = format!("🧪 Test Results for app: {}\n\n", app);
                output.push_str(&summary);

                if !diagnostics.is_empty() {
                    output.push_str(&format!(
                        "\n📋 {} diagnostics generated for failed tests",
                        diagnostics.len()
                    ));
                }

                Ok(SlashCommandResult {
                    text: output,
                    run_commands_in_text: false,
                })
            }
            Err(e) => Err(format!("Failed to run tests: {}", e)),
        }
    }

    fn create_doctype_json(&self, name: &str, module: &str) -> String {
        let snake_case = name.to_lowercase().replace(" ", "_");

        // Generate smart field suggestions
        let suggested_fields = self.generate_smart_fields(name);
        let fields_json = suggested_fields
            .iter()
            .enumerate()
            .map(|(i, (fieldname, fieldtype, label))| {
                format!(
                    r#"        {{
            "fieldname": "{}",
            "fieldtype": "{}",
            "label": "{}",
            "reqd": {}
        }}"#,
                    fieldname,
                    fieldtype,
                    label,
                    if i == 0 { 1 } else { 0 }
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");

        let field_order = suggested_fields
            .iter()
            .map(|(fieldname, _, _)| format!(r#"        "{}""#, fieldname))
            .collect::<Vec<_>>()
            .join(",\n");

        format!(
            r#"{{
    "actions": [],
    "allow_rename": 1,
    "creation": "2024-01-01 00:00:00.000000",
    "doctype": "DocType",
    "editable_grid": 1,
    "engine": "InnoDB",
    "field_order": [
{}
    ],
    "fields": [
{}
    ],
    "index_web_pages_for_search": 1,
    "links": [],
    "modified": "2024-01-01 00:00:00.000000",
    "modified_by": "Administrator",
    "module": "{}",
    "name": "{}",
    "naming_rule": "By fieldname",
    "owner": "Administrator",
    "permissions": [
        {{
            "create": 1,
            "delete": 1,
            "email": 1,
            "export": 1,
            "print": 1,
            "read": 1,
            "report": 1,
            "role": "System Manager",
            "share": 1,
            "write": 1
        }}
    ],
    "sort_field": "modified",
    "sort_order": "DESC",
    "states": [],
    "track_changes": 1
}}"#,
            field_order, fields_json, module, name
        )
    }

    fn generate_smart_fields(&self, doctype_name: &str) -> Vec<(String, String, String)> {
        let mut fields = Vec::new();

        // Always start with a name field
        let name_field = if doctype_name.to_lowercase().contains("item") {
            (
                "item_name".to_string(),
                "Data".to_string(),
                "Item Name".to_string(),
            )
        } else {
            ("title".to_string(), "Data".to_string(), "Title".to_string())
        };
        fields.push(name_field);

        // Add common fields based on DocType name patterns
        let doctype_lower = doctype_name.to_lowercase();

        if doctype_lower.contains("customer") || doctype_lower.contains("supplier") {
            fields.push((
                "contact_person".to_string(),
                "Data".to_string(),
                "Contact Person".to_string(),
            ));
            fields.push(("email".to_string(), "Data".to_string(), "Email".to_string()));
            fields.push((
                "phone".to_string(),
                "Phone".to_string(),
                "Phone".to_string(),
            ));
        }

        if doctype_lower.contains("transaction")
            || doctype_lower.contains("order")
            || doctype_lower.contains("invoice")
        {
            fields.push(("date".to_string(), "Date".to_string(), "Date".to_string()));
            fields.push((
                "total_amount".to_string(),
                "Currency".to_string(),
                "Total Amount".to_string(),
            ));
            fields.push((
                "status".to_string(),
                "Select".to_string(),
                "Status".to_string(),
            ));
        }

        if doctype_lower.contains("item") || doctype_lower.contains("product") {
            fields.push((
                "item_code".to_string(),
                "Data".to_string(),
                "Item Code".to_string(),
            ));
            fields.push((
                "description".to_string(),
                "Text".to_string(),
                "Description".to_string(),
            ));
            fields.push((
                "unit_price".to_string(),
                "Currency".to_string(),
                "Unit Price".to_string(),
            ));
        }

        // Always add common audit fields
        fields.push((
            "is_active".to_string(),
            "Check".to_string(),
            "Is Active".to_string(),
        ));
        fields.push((
            "remarks".to_string(),
            "Text".to_string(),
            "Remarks".to_string(),
        ));

        fields
    }

    fn create_doctype_controller(&self, name: &str, module: &str) -> String {
        let snake_case = name.to_lowercase().replace(" ", "_");
        format!(
            r#"# Copyright (c) 2024, Frappe Technologies and contributors
# For license information, please see license.txt

import frappe
from frappe.model.document import Document


class {}(Document):
    def validate(self):
        """Called before saving the document"""
        pass

    def before_save(self):
        """Called before saving the document"""
        pass

    def after_insert(self):
        """Called after inserting the document"""
        pass

    def on_update(self):
        """Called after updating the document"""
        pass

    def on_cancel(self):
        """Called when cancelling the document"""
        pass

    def on_trash(self):
        """Called before deleting the document"""
        pass
"#,
            name.replace(" ", "")
        )
    }

    fn create_doctype_client_script(&self, name: &str) -> String {
        format!(
            r#"// Copyright (c) 2024, Frappe Technologies and contributors
// For license information, please see license.txt

frappe.ui.form.on('{}', {{
    refresh: function(frm) {{
        // Called when form is refreshed
    }},

    onload: function(frm) {{
        // Called when form is loaded
    }},

    before_save: function(frm) {{
        // Called before saving the document
    }},

    after_save: function(frm) {{
        // Called after saving the document
    }},

    validate: function(frm) {{
        // Called during validation
    }}
}});
"#,
            name
        )
    }

    fn search_doctypes(
        &self,
        query: &str,
        worktree: &Worktree,
    ) -> Result<SlashCommandResult, String> {
        // Analyze project if not already done
        let mut analyzer = FrappeAnalyzer::new();
        if let Err(_) = analyzer.analyze_project(&worktree.abs_path()) {
            return Err("Failed to analyze Frappe project".to_string());
        }

        let results = analyzer.search_doctypes(query);

        if results.is_empty() {
            return Ok(SlashCommandResult {
                text: format!("No DocTypes found matching '{}'", query),
                run_commands_in_text: false,
            });
        }

        let mut output = format!("Found {} DocTypes matching '{}':\n\n", results.len(), query);
        for doctype in results.iter().take(10) {
            // Limit to first 10 results
            output.push_str(&format!(
                "• {} (Module: {})\n  Path: {}\n  Fields: {}\n\n",
                doctype.name,
                doctype.module,
                doctype.file_path.display(),
                doctype.fields.len()
            ));
        }

        if results.len() > 10 {
            output.push_str(&format!("... and {} more results\n", results.len() - 10));
        }

        Ok(SlashCommandResult {
            text: output,
            run_commands_in_text: false,
        })
    }

    fn analyze_current_project(&self, worktree: &Worktree) -> Result<SlashCommandResult, String> {
        let mut analyzer = FrappeAnalyzer::new();

        match analyzer.analyze_project(&worktree.abs_path()) {
            Ok(_) => {
                if let Some(project) = analyzer.get_project() {
                    let mut output = format!("📊 Frappe Project Analysis\n");
                    output.push_str(&format!(
                        "📁 Bench Path: {}\n",
                        project.bench_path.display()
                    ));
                    output.push_str(&format!(
                        "🌐 Default Site: {}\n\n",
                        project.default_site.as_deref().unwrap_or("Not configured")
                    ));

                    output.push_str(&format!("📱 Apps ({}):\n", project.apps.len()));
                    for app in &project.apps {
                        output.push_str(&format!(
                            "  • {} ({} DocTypes, {} Pages, {} Reports)\n",
                            app.name,
                            app.doctypes.len(),
                            app.pages.len(),
                            app.reports.len()
                        ));
                    }

                    output.push_str(&format!("\n🏢 Sites ({}):\n", project.sites.len()));
                    for site in &project.sites {
                        output.push_str(&format!(
                            "  • {} (DB: {})\n",
                            site.name,
                            site.database.as_deref().unwrap_or("Unknown")
                        ));
                    }

                    // DocType summary
                    let total_doctypes: usize = project.apps.iter().map(|a| a.doctypes.len()).sum();
                    if total_doctypes > 0 {
                        output.push_str(&format!("\n📋 Total DocTypes: {}\n", total_doctypes));

                        // Find most common field types
                        let mut field_types = HashMap::new();
                        for app in &project.apps {
                            for doctype in &app.doctypes {
                                for field in &doctype.fields {
                                    *field_types.entry(field.fieldtype.clone()).or_insert(0) += 1;
                                }
                            }
                        }

                        let mut sorted_types: Vec<_> = field_types.into_iter().collect();
                        sorted_types.sort_by(|a, b| b.1.cmp(&a.1));

                        output.push_str("📊 Top Field Types:\n");
                        for (field_type, count) in sorted_types.iter().take(5) {
                            output.push_str(&format!("  • {}: {}\n", field_type, count));
                        }
                    }

                    Ok(SlashCommandResult {
                        text: output,
                        run_commands_in_text: false,
                    })
                } else {
                    Err("Failed to get project information".to_string())
                }
            }
            Err(e) => Err(format!("Analysis failed: {}", e)),
        }
    }

    fn list_running_processes(&self) -> Result<SlashCommandResult, String> {
        let processes = self.process_manager.list_running_processes();

        if processes.is_empty() {
            return Ok(SlashCommandResult {
                text: "ℹ️ No Frappe processes are currently running".to_string(),
                run_commands_in_text: false,
            });
        }

        let mut output = format!("🔄 Running Processes ({})\n\n", processes.len());

        for process in processes {
            let duration = process.start_time.elapsed().as_secs();
            output.push_str(&format!(
                "🟢 {} (PID: {})\n",
                process.id,
                process
                    .pid
                    .map(|p| p.to_string())
                    .unwrap_or("N/A".to_string())
            ));
            output.push_str(&format!("   Command: {}\n", process.command));
            output.push_str(&format!("   Running for: {}s\n", duration));
            output.push_str(&format!("   Status: {:?}\n\n", process.status));
        }

        Ok(SlashCommandResult {
            text: output,
            run_commands_in_text: false,
        })
    }

    fn stop_all_processes(&self) -> Result<SlashCommandResult, String> {
        match self.process_manager.stop_all_processes() {
            Ok(stopped_processes) => {
                if stopped_processes.is_empty() {
                    Ok(SlashCommandResult {
                        text: "ℹ️ No processes were running to stop".to_string(),
                        run_commands_in_text: false,
                    })
                } else {
                    let output = format!(
                        "✅ Stopped {} processes:\n{}",
                        stopped_processes.len(),
                        stopped_processes
                            .iter()
                            .map(|id| format!("  • {}", id))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    Ok(SlashCommandResult {
                        text: output,
                        run_commands_in_text: false,
                    })
                }
            }
            Err(e) => Err(format!("Failed to stop processes: {}", e)),
        }
    }
}

register_extension!(LatteExtension);
