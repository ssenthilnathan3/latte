use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::process::{ChildStderr, ChildStdout};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: String,
    pub status: ProcessStatus,
    pub pid: Option<u32>,
    pub start_time: SystemTime,
    pub output_lines: Vec<String>,
    pub error_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProcessStatus {
    Starting,
    Running,
    Stopped,
    Failed,
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub timestamp: SystemTime,
    pub level: LogLevel,
    pub content: String,
    pub source: LogSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogSource {
    Stdout,
    Stderr,
    System,
}

pub struct ProcessManager {
    processes: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    log_buffer_size: usize,
}

struct ProcessHandle {
    info: ProcessInfo,
    child: Option<Child>,
    log_lines: Vec<LogLine>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            log_buffer_size: 1000, // Keep last 1000 log lines per process
        }
    }

    pub fn start_bench_process(
        &self,
        id: String,
        bench_path: &str,
        command: &str,
        args: Vec<String>,
    ) -> Result<String, String> {
        let full_command = format!("bench {}", command);
        let mut cmd_args = vec![command.to_string()];
        cmd_args.extend(args.clone());

        let mut child = Command::new("bench")
            .args(&cmd_args)
            .current_dir(bench_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start bench process: {}", e))?;

        let pid = child.id();

        let process_info = ProcessInfo {
            id: id.clone(),
            command: full_command,
            args: cmd_args,
            working_dir: bench_path.to_string(),
            status: ProcessStatus::Starting,
            start_time: SystemTime::now(),
            pid: Some(pid),
            output_lines: Vec::new(),
            error_lines: Vec::new(),
        };

        // Start monitoring threads before moving child into ProcessHandle
        self.start_output_monitoring(&id, child);

        // Store the process after monitoring is set up
        let process_handle = ProcessHandle {
            info: process_info,
            child: None, // Child is consumed by start_output_monitoring; process_monitoring will check status
            log_lines: Vec::new(),
        };

        {
            let mut processes = self.processes.lock().unwrap();
            processes.insert(id.clone(), process_handle);
        }

        // Start process monitoring
        self.start_process_monitoring(&id);

        Ok(id)
    }

    pub fn start_simple_command(
        &self,
        id: String,
        working_dir: &str,
        command: &str,
        args: Vec<String>,
    ) -> Result<String, String> {
        let mut child = Command::new(command)
            .args(&args)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start command: {}", e))?;

        let pid = child.id();

        let process_info = ProcessInfo {
            id: id.clone(),
            command: command.to_string(),
            args,
            working_dir: working_dir.to_string(),
            status: ProcessStatus::Starting,
            start_time: SystemTime::now(),
            pid: Some(pid),
            output_lines: Vec::new(),
            error_lines: Vec::new(),
        };

        // Start monitoring threads before moving child into ProcessHandle
        self.start_output_monitoring(&id, child);

        // Store the process after monitoring is set up
        let process_handle = ProcessHandle {
            info: process_info,
            child: None, // Child is consumed by start_output_monitoring
            log_lines: Vec::new(),
        };

        {
            let mut processes = self.processes.lock().unwrap();
            processes.insert(id.clone(), process_handle);
        }

        // Start process monitoring
        self.start_process_monitoring(&id);

        Ok(id)
    }

    fn start_output_monitoring(&self, process_id: &str, child: Child) {
        let processes_ref = Arc::clone(&self.processes);
        let id = process_id.to_string();
        let buffer_size = self.log_buffer_size;

        // Store the child in ProcessHandle
        {
            let mut processes = processes_ref.lock().unwrap();
            if let Some(handle) = processes.get_mut(&id) {
                handle.child = Some(child);
            }
        }

        // Clone Arc for stdout monitoring
        let stdout_processes = Arc::clone(&processes_ref);
        let stdout_id = id.clone();
        thread::spawn(move || {
            if let Some(mut child) = {
                let mut processes = stdout_processes.lock().unwrap();
                processes
                    .get_mut(&stdout_id)
                    .and_then(|handle| handle.child.take())
            } {
                ProcessManager::monitor_stream(
                    &stdout_processes,
                    &stdout_id,
                    LogSource::Stdout,
                    &mut child,
                    buffer_size,
                );
                // Restore child
                let mut processes = stdout_processes.lock().unwrap();
                if let Some(handle) = processes.get_mut(&stdout_id) {
                    handle.child = Some(child);
                }
            }
        });

        // Clone Arc for stderr monitoring
        let stderr_processes = Arc::clone(&processes_ref);
        let stderr_id = id.clone();
        thread::spawn(move || {
            if let Some(mut child) = {
                let mut processes = stderr_processes.lock().unwrap();
                processes
                    .get_mut(&stderr_id)
                    .and_then(|handle| handle.child.take())
            } {
                ProcessManager::monitor_stream(
                    &stderr_processes,
                    &stderr_id,
                    LogSource::Stderr,
                    &mut child,
                    buffer_size,
                );
                // Restore child
                let mut processes = stderr_processes.lock().unwrap();
                if let Some(handle) = processes.get_mut(&stderr_id) {
                    handle.child = Some(child);
                }
            }
        });
    }

    fn monitor_stream(
        processes: &Arc<Mutex<HashMap<String, ProcessHandle>>>,
        process_id: &String,
        source: LogSource,
        child: &mut Child,
        buffer_size: usize,
    ) {
        let reader: Option<Box<dyn BufRead>> = match source {
            LogSource::Stdout => {
                if let Some(stdout) = child.stdout.take() {
                    Some(Box::new(BufReader::new(stdout)) as Box<dyn BufRead>)
                } else {
                    None
                }
            }
            LogSource::Stderr => {
                if let Some(stderr) = child.stderr.take() {
                    Some(Box::new(BufReader::new(stderr)) as Box<dyn BufRead>)
                } else {
                    None
                }
            }
            LogSource::System => {
                None // System logs not handled via child process
            }
        };

        if let Some(mut reader) = reader {
            let mut line = String::new();
            loop {
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let log_line = LogLine {
                            timestamp: SystemTime::now(),
                            level: ProcessManager::detect_log_level(&line),
                            content: line.trim_end().to_string(),
                            source: source.clone(),
                        };

                        // Add to process logs
                        let mut proc_map = processes.lock().unwrap();
                        if let Some(handle) = proc_map.get_mut(process_id) {
                            handle.log_lines.push(log_line);

                            // Also add to the info for quick access
                            match source {
                                LogSource::Stdout => {
                                    handle.info.output_lines.push(line.trim_end().to_string());
                                }
                                LogSource::Stderr => {
                                    handle.info.error_lines.push(line.trim_end().to_string());
                                }
                                LogSource::System => {
                                    handle.info.output_lines.push(line.trim_end().to_string());
                                }
                            }

                            // Keep buffer size manageable
                            if handle.log_lines.len() > buffer_size {
                                handle.log_lines.remove(0);
                            }
                            if handle.info.output_lines.len() > buffer_size {
                                handle.info.output_lines.remove(0);
                            }
                            if handle.info.error_lines.len() > buffer_size {
                                handle.info.error_lines.remove(0);
                            }
                        }
                        line.clear();
                    }
                    Err(_) => break,
                }
            }
        }
    }

    fn start_process_monitoring(&self, process_id: &str) {
        let processes_ref = Arc::clone(&self.processes);
        let id = process_id.to_string();

        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(1));

                let should_continue = {
                    let mut proc_map = processes_ref.lock().unwrap();
                    if let Some(handle) = proc_map.get_mut(&id) {
                        if let Some(ref mut child) = handle.child {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    handle.info.status = if status.success() {
                                        ProcessStatus::Stopped
                                    } else {
                                        ProcessStatus::Failed
                                    };
                                    handle.child = None;
                                    false // Stop monitoring
                                }
                                Ok(None) => {
                                    if handle.info.status == ProcessStatus::Starting {
                                        handle.info.status = ProcessStatus::Running;
                                    }
                                    true // Continue monitoring
                                }
                                Err(_) => {
                                    handle.info.status = ProcessStatus::Failed;
                                    handle.child = None;
                                    false // Stop monitoring
                                }
                            }
                        } else {
                            false // No child process, stop monitoring
                        }
                    } else {
                        false // Process not found, stop monitoring
                    }
                };

                if !should_continue {
                    break;
                }
            }
        });
    }

    pub fn stop_process(&self, process_id: &str) -> Result<(), String> {
        let mut proc_map = self.processes.lock().unwrap();
        if let Some(handle) = proc_map.get_mut(process_id) {
            if let Some(ref mut child) = handle.child {
                child
                    .kill()
                    .map_err(|e| format!("Failed to kill process: {}", e))?;
                handle.info.status = ProcessStatus::Killed;
                handle.child = None;
                Ok(())
            } else {
                Err("Process is not running".to_string())
            }
        } else {
            Err("Process not found".to_string())
        }
    }

    pub fn get_process_info(&self, process_id: &str) -> Option<ProcessInfo> {
        let proc_map = self.processes.lock().unwrap();
        proc_map.get(process_id).map(|handle| handle.info.clone())
    }

    pub fn get_process_logs(&self, process_id: &str) -> Vec<LogLine> {
        let proc_map = self.processes.lock().unwrap();
        proc_map
            .get(process_id)
            .map(|handle| handle.log_lines.clone())
            .unwrap_or_default()
    }

    pub fn get_recent_logs(&self, process_id: &str, count: usize) -> Vec<LogLine> {
        let proc_map = self.processes.lock().unwrap();
        if let Some(handle) = proc_map.get(process_id) {
            let start = if handle.log_lines.len() > count {
                handle.log_lines.len() - count
            } else {
                0
            };
            handle.log_lines[start..].to_vec()
        } else {
            Vec::new()
        }
    }

    pub fn list_processes(&self) -> Vec<ProcessInfo> {
        let proc_map = self.processes.lock().unwrap();
        proc_map
            .values()
            .map(|handle| handle.info.clone())
            .collect()
    }

    pub fn list_running_processes(&self) -> Vec<ProcessInfo> {
        let proc_map = self.processes.lock().unwrap();
        proc_map
            .values()
            .filter(|handle| {
                matches!(
                    handle.info.status,
                    ProcessStatus::Starting | ProcessStatus::Running
                )
            })
            .map(|handle| handle.info.clone())
            .collect()
    }

    pub fn cleanup_finished_processes(&self) {
        let mut proc_map = self.processes.lock().unwrap();
        proc_map.retain(|_, handle| {
            !matches!(
                handle.info.status,
                ProcessStatus::Stopped | ProcessStatus::Failed | ProcessStatus::Killed
            )
        });
    }

    pub fn stop_all_processes(&self) -> Result<Vec<String>, String> {
        let mut stopped_processes = Vec::new();
        let process_ids: Vec<String> = {
            let proc_map = self.processes.lock().unwrap();
            proc_map.keys().cloned().collect()
        };

        for process_id in process_ids {
            if let Ok(()) = self.stop_process(&process_id) {
                stopped_processes.push(process_id);
            }
        }

        Ok(stopped_processes)
    }

    pub fn is_bench_running(&self) -> bool {
        let proc_map = self.processes.lock().unwrap();
        proc_map.values().any(|handle| {
            handle.info.command.contains("bench start")
                && matches!(
                    handle.info.status,
                    ProcessStatus::Starting | ProcessStatus::Running
                )
        })
    }

    pub fn get_bench_process_id(&self) -> Option<String> {
        let proc_map = self.processes.lock().unwrap();
        proc_map
            .values()
            .find(|handle| {
                handle.info.command.contains("bench start")
                    && matches!(
                        handle.info.status,
                        ProcessStatus::Starting | ProcessStatus::Running
                    )
            })
            .map(|handle| handle.info.id.clone())
    }

    fn detect_log_level(line: &str) -> LogLevel {
        let line_lower = line.to_lowercase();

        if line_lower.contains("error") || line_lower.contains("exception") {
            LogLevel::Error
        } else if line_lower.contains("warning") || line_lower.contains("warn") {
            LogLevel::Warning
        } else if line_lower.contains("debug") {
            LogLevel::Debug
        } else if line_lower.contains("trace") {
            LogLevel::Trace
        } else {
            LogLevel::Info
        }
    }

    pub fn format_logs_for_display(&self, process_id: &str) -> String {
        let logs = self.get_process_logs(process_id);
        let mut output = String::new();

        for log in logs {
            let level_icon = match log.level {
                LogLevel::Error => "🔥",
                LogLevel::Warning => "⚠️",
                LogLevel::Info => "ℹ️",
                LogLevel::Debug => "🐛",
                LogLevel::Trace => "🔍",
            };

            let source_prefix = match log.source {
                LogSource::Stdout => "",
                LogSource::Stderr => "[ERR] ",
                LogSource::System => "[SYS] ",
            };

            output.push_str(&format!(
                "{} {}{}\n",
                level_icon, source_prefix, log.content
            ));
        }

        output
    }

    pub fn extract_clickable_errors(&self, process_id: &str) -> Vec<ClickableError> {
        let logs = self.get_process_logs(process_id);
        let mut errors = Vec::new();

        for log in logs {
            if log.level == LogLevel::Error {
                if let Some(error) = self.parse_error_line(&log.content) {
                    errors.push(error);
                }
            }
        }

        errors
    }

    fn parse_error_line(&self, line: &str) -> Option<ClickableError> {
        // Pattern to match Python traceback file references
        let file_pattern = Regex::new(r#"File "([^"]+)", line (\d+)"#).ok()?;

        if let Some(captures) = file_pattern.captures(line) {
            let file_path = captures.get(1)?.as_str().to_string();
            let line_number = captures.get(2)?.as_str().parse::<u32>().ok()?;

            return Some(ClickableError {
                file_path,
                line_number,
                message: line.to_string(),
                error_type: ErrorType::PythonTraceback,
            });
        }

        // Pattern to match JavaScript/Node.js errors
        let js_pattern = Regex::new(r"at ([^(]+) \(([^:]+):(\d+):(\d+)\)").ok()?;

        if let Some(captures) = js_pattern.captures(line) {
            let file_path = captures.get(2)?.as_str().to_string();
            let line_number = captures.get(3)?.as_str().parse::<u32>().ok()?;

            return Some(ClickableError {
                file_path,
                line_number,
                message: line.to_string(),
                error_type: ErrorType::JavaScriptError,
            });
        }

        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickableError {
    pub file_path: String,
    pub line_number: u32,
    pub message: String,
    pub error_type: ErrorType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorType {
    PythonTraceback,
    JavaScriptError,
    BuildError,
    TestFailure,
    Generic,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

// Utility functions for bench-specific operations
impl ProcessManager {
    pub fn start_bench_dev_server(&self, bench_path: &str) -> Result<String, String> {
        let process_id = format!("bench_start_{}", chrono::Utc::now().timestamp());
        self.start_bench_process(process_id.clone(), bench_path, "start", vec![])
    }

    pub fn run_bench_migrate(
        &self,
        bench_path: &str,
        site: Option<&str>,
    ) -> Result<String, String> {
        let process_id = format!("bench_migrate_{}", chrono::Utc::now().timestamp());
        let mut args = vec![];

        if let Some(site_name) = site {
            args.extend(vec!["--site".to_string(), site_name.to_string()]);
        }

        args.push("migrate".to_string());
        self.start_bench_process(process_id.clone(), bench_path, "migrate", args)
    }

    pub fn run_bench_build(&self, bench_path: &str) -> Result<String, String> {
        let process_id = format!("bench_build_{}", chrono::Utc::now().timestamp());
        self.start_bench_process(process_id.clone(), bench_path, "build", vec![])
    }

    pub fn create_new_app(&self, bench_path: &str, app_name: &str) -> Result<String, String> {
        let process_id = format!("bench_new_app_{}", chrono::Utc::now().timestamp());
        self.start_bench_process(
            process_id.clone(),
            bench_path,
            "new-app",
            vec![app_name.to_string()],
        )
    }

    pub fn create_new_site(&self, bench_path: &str, site_name: &str) -> Result<String, String> {
        let process_id = format!("bench_new_site_{}", chrono::Utc::now().timestamp());
        self.start_bench_process(
            process_id.clone(),
            bench_path,
            "new-site",
            vec![site_name.to_string()],
        )
    }

    pub fn open_console(&self, bench_path: &str, site: &str) -> Result<String, String> {
        let process_id = format!("bench_console_{}", chrono::Utc::now().timestamp());
        self.start_bench_process(
            process_id.clone(),
            bench_path,
            "console",
            vec!["--site".to_string(), site.to_string()],
        )
    }

    pub fn open_mariadb(&self, bench_path: &str, site: &str) -> Result<String, String> {
        let process_id = format!("bench_mariadb_{}", chrono::Utc::now().timestamp());
        self.start_bench_process(
            process_id.clone(),
            bench_path,
            "mariadb",
            vec!["--site".to_string(), site.to_string()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_process_manager_creation() {
        let manager = ProcessManager::new();
        assert_eq!(manager.list_processes().len(), 0);
    }

    #[test]
    fn test_detect_log_level() {
        assert_eq!(
            ProcessManager::detect_log_level("ERROR: Something went wrong"),
            LogLevel::Error
        );
        assert_eq!(
            ProcessManager::detect_log_level("Warning: This is a warning"),
            LogLevel::Warning
        );
        assert_eq!(
            ProcessManager::detect_log_level("Info: Normal message"),
            LogLevel::Info
        );
    }

    #[test]
    fn test_parse_error_line() {
        let manager = ProcessManager::new();
        let line = r#"File "/path/to/file.py", line 42"#;

        let error = manager.parse_error_line(line);
        assert!(error.is_some());

        let error = error.unwrap();
        assert_eq!(error.file_path, "/path/to/file.py");
        assert_eq!(error.line_number, 42);
    }
}
