use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub test_name: String,
    pub module: String,
    pub app: String,
    pub status: TestStatus,
    pub duration: f64,
    pub error_message: Option<String>,
    pub traceback: Option<Vec<String>>,
    pub line_number: Option<u32>,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TestStatus {
    Passed,
    Failed,
    Error,
    Skipped,
    Running,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuite {
    pub name: String,
    pub app: String,
    pub total_tests: u32,
    pub passed: u32,
    pub failed: u32,
    pub errors: u32,
    pub skipped: u32,
    pub duration: f64,
    pub results: Vec<TestResult>,
}

#[derive(Debug, Clone)]
pub struct TestRunner {
    bench_path: String,
    site_name: String,
    running_tests: Arc<Mutex<HashMap<String, bool>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPattern {
    pub pattern: String,
    pub description: String,
    pub severity: DiagnosticSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file_path: String,
    pub line_number: u32,
    pub column: Option<u32>,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub code: Option<String>,
    pub source: String,
    pub related_info: Vec<DiagnosticRelatedInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticRelatedInfo {
    pub file_path: String,
    pub line_number: u32,
    pub message: String,
}

impl TestRunner {
    pub fn new(bench_path: String, site_name: String) -> Self {
        Self {
            bench_path,
            site_name,
            running_tests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn run_app_tests(&self, app_name: &str) -> Result<TestSuite, String> {
        let test_id = format!("{}::{}", app_name, chrono::Utc::now().timestamp());

        // Mark test as running
        {
            let mut running = self.running_tests.lock().unwrap();
            running.insert(test_id.clone(), true);
        }

        let result = self.execute_tests(app_name);

        // Mark test as finished
        {
            let mut running = self.running_tests.lock().unwrap();
            running.remove(&test_id);
        }

        result
    }

    pub fn run_specific_test(&self, app_name: &str, test_path: &str) -> Result<TestResult, String> {
        let command = format!(
            "cd {} && bench --site {} run-tests --app {} --test {}",
            self.bench_path, self.site_name, app_name, test_path
        );

        let output = Command::new("bash")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Failed to run test command: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        self.parse_single_test_result(test_path, &stdout, &stderr)
    }

    fn execute_tests(&self, app_name: &str) -> Result<TestSuite, String> {
        let command = format!(
            "cd {} && bench --site {} run-tests --app {} --verbose",
            self.bench_path, self.site_name, app_name
        );

        let output = Command::new("bash")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Failed to run test command: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        self.parse_test_output(app_name, &stdout, &stderr)
    }

    fn parse_test_output(
        &self,
        app_name: &str,
        stdout: &str,
        stderr: &str,
    ) -> Result<TestSuite, String> {
        let mut test_suite = TestSuite {
            name: app_name.to_string(),
            app: app_name.to_string(),
            total_tests: 0,
            passed: 0,
            failed: 0,
            errors: 0,
            skipped: 0,
            duration: 0.0,
            results: Vec::new(),
        };

        // Parse test results from output
        let test_patterns = self.get_test_patterns();

        // Parse individual test results
        for line in stdout.lines().chain(stderr.lines()) {
            if let Some(test_result) = self.parse_test_line(line, app_name, &test_patterns) {
                match test_result.status {
                    TestStatus::Passed => test_suite.passed += 1,
                    TestStatus::Failed => test_suite.failed += 1,
                    TestStatus::Error => test_suite.errors += 1,
                    TestStatus::Skipped => test_suite.skipped += 1,
                    _ => {}
                }
                test_suite.results.push(test_result);
            }
        }

        test_suite.total_tests =
            test_suite.passed + test_suite.failed + test_suite.errors + test_suite.skipped;

        // Parse execution time
        if let Some(duration) = self.extract_duration(stdout) {
            test_suite.duration = duration;
        }

        Ok(test_suite)
    }

    fn parse_single_test_result(
        &self,
        test_path: &str,
        stdout: &str,
        stderr: &str,
    ) -> Result<TestResult, String> {
        let test_patterns = self.get_test_patterns();

        // Try to parse from stdout first, then stderr
        let all_output = format!("{}\n{}", stdout, stderr);

        for line in all_output.lines() {
            if let Some(result) = self.parse_test_line(line, "", &test_patterns) {
                return Ok(TestResult {
                    test_name: test_path.to_string(),
                    ..result
                });
            }
        }

        // If no specific result found, determine status from exit code and output
        let status = if stderr.contains("FAILED") || stderr.contains("ERROR") {
            TestStatus::Failed
        } else if stdout.contains("OK") || stdout.contains("PASSED") {
            TestStatus::Passed
        } else {
            TestStatus::Error
        };

        Ok(TestResult {
            test_name: test_path.to_string(),
            module: "".to_string(),
            app: "".to_string(),
            status,
            duration: 0.0,
            error_message: if !stderr.is_empty() {
                Some(stderr.to_string())
            } else {
                None
            },
            traceback: None,
            line_number: None,
            file_path: Some(test_path.to_string()),
        })
    }

    fn parse_test_line(
        &self,
        line: &str,
        app_name: &str,
        patterns: &[TestPattern],
    ) -> Option<TestResult> {
        // Pattern for pytest-style output
        let pytest_re = Regex::new(
            r"^(.+)::\s*(\w+)\s*::\s*(\w+)\s*(PASSED|FAILED|ERROR|SKIPPED)(?:\s*\[(\d+\.\d+)s\])?",
        )
        .ok()?;

        if let Some(captures) = pytest_re.captures(line) {
            let file_path = captures.get(1)?.as_str().to_string();
            let class_name = captures.get(2)?.as_str().to_string();
            let test_name = captures.get(3)?.as_str().to_string();
            let status_str = captures.get(4)?.as_str();
            let duration = captures
                .get(5)
                .and_then(|m| m.as_str().parse::<f64>().ok())
                .unwrap_or(0.0);

            let status = match status_str {
                "PASSED" => TestStatus::Passed,
                "FAILED" => TestStatus::Failed,
                "ERROR" => TestStatus::Error,
                "SKIPPED" => TestStatus::Skipped,
                _ => TestStatus::Error,
            };

            return Some(TestResult {
                test_name: format!("{}::{}", class_name, test_name),
                module: class_name,
                app: app_name.to_string(),
                status,
                duration,
                error_message: None,
                traceback: None,
                line_number: None,
                file_path: Some(file_path),
            });
        }

        // Pattern for unittest-style output
        let unittest_re =
            Regex::new(r"^(\w+)\s+\(([^.]+)\.(\w+)\)\s+\.\.\.\s+(ok|FAIL|ERROR|skip)").ok()?;

        if let Some(captures) = unittest_re.captures(line) {
            let test_name = captures.get(1)?.as_str().to_string();
            let module_name = captures.get(2)?.as_str().to_string();
            let class_name = captures.get(3)?.as_str().to_string();
            let status_str = captures.get(4)?.as_str();

            let status = match status_str {
                "ok" => TestStatus::Passed,
                "FAIL" => TestStatus::Failed,
                "ERROR" => TestStatus::Error,
                "skip" => TestStatus::Skipped,
                _ => TestStatus::Error,
            };

            return Some(TestResult {
                test_name,
                module: format!("{}.{}", module_name, class_name),
                app: app_name.to_string(),
                status,
                duration: 0.0,
                error_message: None,
                traceback: None,
                line_number: None,
                file_path: None,
            });
        }

        None
    }

    fn extract_duration(&self, output: &str) -> Option<f64> {
        // Pattern for total execution time
        let duration_patterns = [
            r"Ran \d+ tests? in ([\d.]+)s",
            r"=+ [\d.]+ seconds =+",
            r"Total time: ([\d.]+)s",
        ];

        for pattern in &duration_patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(captures) = re.captures(output) {
                    if let Some(duration_str) = captures.get(1) {
                        if let Ok(duration) = duration_str.as_str().parse::<f64>() {
                            return Some(duration);
                        }
                    }
                }
            }
        }

        None
    }

    fn get_test_patterns(&self) -> Vec<TestPattern> {
        vec![
            TestPattern {
                pattern: r"FAILED.*AssertionError".to_string(),
                description: "Assertion failure in test".to_string(),
                severity: DiagnosticSeverity::Error,
            },
            TestPattern {
                pattern: r"ERROR.*ImportError".to_string(),
                description: "Import error in test".to_string(),
                severity: DiagnosticSeverity::Error,
            },
            TestPattern {
                pattern: r"WARNING".to_string(),
                description: "Test warning".to_string(),
                severity: DiagnosticSeverity::Warning,
            },
            TestPattern {
                pattern: r"DeprecationWarning".to_string(),
                description: "Deprecated API usage".to_string(),
                severity: DiagnosticSeverity::Warning,
            },
        ]
    }

    pub fn parse_traceback(&self, error_output: &str) -> Vec<String> {
        let mut traceback = Vec::new();
        let mut in_traceback = false;

        for line in error_output.lines() {
            if line.starts_with("Traceback") {
                in_traceback = true;
            }

            if in_traceback {
                traceback.push(line.to_string());

                // End of traceback is usually an exception line
                if !line.starts_with("  ")
                    && !line.starts_with("Traceback")
                    && !line.trim().is_empty()
                {
                    break;
                }
            }
        }

        traceback
    }

    pub fn extract_diagnostics(&self, test_results: &[TestResult]) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for result in test_results {
            match &result.status {
                TestStatus::Failed | TestStatus::Error => {
                    if let Some(error_msg) = &result.error_message {
                        let diagnostic = self.create_diagnostic_from_error(result, error_msg);
                        diagnostics.push(diagnostic);
                    }
                }
                _ => {}
            }
        }

        diagnostics
    }

    fn create_diagnostic_from_error(
        &self,
        test_result: &TestResult,
        error_message: &str,
    ) -> Diagnostic {
        let (line_number, file_path) = self.extract_error_location(error_message);

        Diagnostic {
            file_path: file_path
                .unwrap_or_else(|| test_result.file_path.clone().unwrap_or_default()),
            line_number: line_number.unwrap_or(1),
            column: None,
            message: self.clean_error_message(error_message),
            severity: if test_result.status == TestStatus::Error {
                DiagnosticSeverity::Error
            } else {
                DiagnosticSeverity::Warning
            },
            code: Some(format!(
                "test_{}",
                test_result.status.to_string().to_lowercase()
            )),
            source: "frappe_test_runner".to_string(),
            related_info: vec![],
        }
    }

    fn extract_error_location(&self, error_message: &str) -> (Option<u32>, Option<String>) {
        // Pattern to match file path and line number from traceback
        let location_re = Regex::new(r#"File "([^"]+)", line (\d+)"#).ok()?;

        if let Some(captures) = location_re.captures(error_message) {
            let file_path = captures.get(1)?.as_str().to_string();
            let line_number = captures.get(2)?.as_str().parse::<u32>().ok()?;
            return (Some(line_number), Some(file_path));
        }

        (None, None)
    }

    fn clean_error_message(&self, error_message: &str) -> String {
        // Extract just the relevant error message, not the full traceback
        let lines: Vec<&str> = error_message.lines().collect();

        // Find the actual error message (usually the last non-empty line)
        for line in lines.iter().rev() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with("File ") && !trimmed.starts_with("  ") {
                return trimmed.to_string();
            }
        }

        // If no clean message found, return first 200 characters
        if error_message.len() > 200 {
            format!("{}...", &error_message[..200])
        } else {
            error_message.to_string()
        }
    }

    pub fn stop_running_tests(&self) -> Result<(), String> {
        // Implementation to stop running test processes
        let running = self.running_tests.lock().unwrap();

        if running.is_empty() {
            return Ok(());
        }

        // In a real implementation, you would track process IDs and kill them
        // For now, we'll just clear the running tests map
        drop(running);

        let mut running = self.running_tests.lock().unwrap();
        running.clear();

        Ok(())
    }

    pub fn get_running_tests(&self) -> Vec<String> {
        let running = self.running_tests.lock().unwrap();
        running.keys().cloned().collect()
    }

    pub fn is_test_running(&self, test_id: &str) -> bool {
        let running = self.running_tests.lock().unwrap();
        running.contains_key(test_id)
    }

    pub fn format_test_summary(&self, test_suite: &TestSuite) -> String {
        let mut summary = format!("📊 Test Results for {}\n", test_suite.app);
        summary.push_str(&format!("⏱️  Duration: {:.2}s\n\n", test_suite.duration));

        summary.push_str(&format!("✅ Passed: {}\n", test_suite.passed));
        summary.push_str(&format!("❌ Failed: {}\n", test_suite.failed));
        summary.push_str(&format!("🔥 Errors: {}\n", test_suite.errors));
        summary.push_str(&format!("⏭️  Skipped: {}\n\n", test_suite.skipped));

        if test_suite.failed > 0 || test_suite.errors > 0 {
            summary.push_str("🚨 Failed/Error Tests:\n");
            for result in &test_suite.results {
                if matches!(result.status, TestStatus::Failed | TestStatus::Error) {
                    summary.push_str(&format!(
                        "  • {} ({})\n",
                        result.test_name,
                        if result.status == TestStatus::Failed {
                            "FAILED"
                        } else {
                            "ERROR"
                        }
                    ));

                    if let Some(error_msg) = &result.error_message {
                        let clean_msg = self.clean_error_message(error_msg);
                        summary.push_str(&format!("    {}\n", clean_msg));
                    }
                }
            }
        }

        summary
    }
}

impl ToString for TestStatus {
    fn to_string(&self) -> String {
        match self {
            TestStatus::Passed => "passed".to_string(),
            TestStatus::Failed => "failed".to_string(),
            TestStatus::Error => "error".to_string(),
            TestStatus::Skipped => "skipped".to_string(),
            TestStatus::Running => "running".to_string(),
            TestStatus::Pending => "pending".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pytest_output() {
        let runner = TestRunner::new("/path/to/bench".to_string(), "test.local".to_string());
        let line = "test_app/test_doctype.py::TestDocType::test_create PASSED [0.123s]";
        let patterns = runner.get_test_patterns();

        let result = runner.parse_test_line(line, "test_app", &patterns);
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.status, TestStatus::Passed);
        assert_eq!(result.duration, 0.123);
        assert_eq!(result.test_name, "TestDocType::test_create");
    }

    #[test]
    fn test_extract_duration() {
        let runner = TestRunner::new("/path/to/bench".to_string(), "test.local".to_string());
        let output = "Ran 15 tests in 2.456s";

        let duration = runner.extract_duration(output);
        assert_eq!(duration, Some(2.456));
    }

    #[test]
    fn test_clean_error_message() {
        let runner = TestRunner::new("/path/to/bench".to_string(), "test.local".to_string());
        let error = "Traceback (most recent call last):\n  File \"test.py\", line 10\n    assert False\nAssertionError: Test failed";

        let clean = runner.clean_error_message(error);
        assert_eq!(clean, "AssertionError: Test failed");
    }
}
