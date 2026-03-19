use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::Local;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimulationTraceLevel {
    Info,
    Warning,
    Error,
}

impl SimulationTraceLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }

    pub fn allows(self, incoming: SimulationTraceLevel) -> bool {
        matches!(
            (self, incoming),
            (SimulationTraceLevel::Info, _)
                | (SimulationTraceLevel::Warning, SimulationTraceLevel::Warning | SimulationTraceLevel::Error)
                | (SimulationTraceLevel::Error, SimulationTraceLevel::Error)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimulationTraceVerbosity {
    Normal,
    Verbose,
}

impl SimulationTraceVerbosity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Verbose => "verbose",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SimulationTraceConfig {
    pub enabled: bool,
    pub level: SimulationTraceLevel,
    pub verbosity: SimulationTraceVerbosity,
    pub write_text: bool,
    pub write_jsonl: bool,
    pub flush_each_event: bool,
}

impl Default for SimulationTraceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            level: SimulationTraceLevel::Info,
            verbosity: SimulationTraceVerbosity::Normal,
            write_text: true,
            write_jsonl: false,
            flush_each_event: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SimulationTraceRunContext {
    pub run_id: String,
    pub output_dir: PathBuf,
    pub grid_summary: String,
    pub workfront_count: usize,
    pub scenario_count: usize,
    pub upper_floor_threshold: f64,
    pub lower_floor_completion_ratio: f64,
    pub lower_floor_forced_completion: usize,
    pub base_seed: u64,
}

#[derive(Clone, Debug)]
pub struct SimulationTraceEvent {
    pub timestamp: String,
    pub level: SimulationTraceLevel,
    pub event_name: String,
    pub scene: Option<usize>,
    pub cycle: Option<usize>,
    pub round: Option<usize>,
    pub wf: Option<i32>,
    pub message: String,
    pub fields: Vec<(String, String)>,
}

impl SimulationTraceEvent {
    pub fn new(
        level: SimulationTraceLevel,
        event_name: impl Into<String>,
        scene: Option<usize>,
        cycle: Option<usize>,
        round: Option<usize>,
        wf: Option<i32>,
        message: impl Into<String>,
        fields: Vec<(String, String)>,
    ) -> Self {
        Self {
            timestamp: timestamp_string(),
            level,
            event_name: event_name.into(),
            scene,
            cycle,
            round,
            wf,
            message: message.into(),
            fields,
        }
    }
}

pub trait TraceSink: Send {
    fn write_event(&mut self, event: &SimulationTraceEvent) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
    fn output_path(&self) -> &Path;
}

pub struct TextTraceSink {
    output_path: PathBuf,
    writer: BufWriter<File>,
}

impl TextTraceSink {
    pub fn create(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(&path)?;
        Ok(Self {
            output_path: path,
            writer: BufWriter::new(file),
        })
    }
}

impl TraceSink for TextTraceSink {
    fn write_event(&mut self, event: &SimulationTraceEvent) -> std::io::Result<()> {
        writeln!(self.writer, "{}", render_text_event(event))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }

    fn output_path(&self) -> &Path {
        &self.output_path
    }
}

pub struct JsonlTraceSink {
    output_path: PathBuf,
    writer: BufWriter<File>,
}

impl JsonlTraceSink {
    pub fn create(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(&path)?;
        Ok(Self {
            output_path: path,
            writer: BufWriter::new(file),
        })
    }
}

impl TraceSink for JsonlTraceSink {
    fn write_event(&mut self, event: &SimulationTraceEvent) -> std::io::Result<()> {
        writeln!(self.writer, "{}", render_jsonl_event(event))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }

    fn output_path(&self) -> &Path {
        &self.output_path
    }
}

pub struct MultiTraceSink {
    sinks: Vec<Box<dyn TraceSink>>,
}

impl MultiTraceSink {
    pub fn new(sinks: Vec<Box<dyn TraceSink>>) -> Self {
        Self { sinks }
    }
}

impl TraceSink for MultiTraceSink {
    fn write_event(&mut self, event: &SimulationTraceEvent) -> std::io::Result<()> {
        for sink in &mut self.sinks {
            sink.write_event(event)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        for sink in &mut self.sinks {
            sink.flush()?;
        }
        Ok(())
    }

    fn output_path(&self) -> &Path {
        self.sinks
            .first()
            .map(|sink| sink.output_path())
            .unwrap_or_else(|| Path::new(""))
    }
}

pub struct SimulationTraceLogger {
    config: SimulationTraceConfig,
    sink: Box<dyn TraceSink>,
    fallback_error_path: PathBuf,
    last_error: Option<String>,
}

impl SimulationTraceLogger {
    pub fn create_for_scene(
        config: SimulationTraceConfig,
        run_context: &SimulationTraceRunContext,
        scene_id: usize,
    ) -> std::io::Result<Self> {
        let mut sinks: Vec<Box<dyn TraceSink>> = Vec::new();

        if config.write_text {
            let text_path = run_context.output_dir.join(format!(
                "sim_trace_run_{}_scene_{:04}.log",
                run_context.run_id, scene_id
            ));
            sinks.push(Box::new(TextTraceSink::create(text_path)?));
        }

        if config.write_jsonl {
            let jsonl_path = run_context.output_dir.join(format!(
                "sim_trace_run_{}_scene_{:04}.jsonl",
                run_context.run_id, scene_id
            ));
            sinks.push(Box::new(JsonlTraceSink::create(jsonl_path)?));
        }

        if sinks.is_empty() {
            let fallback_path = run_context.output_dir.join(format!(
                "sim_trace_run_{}_scene_{:04}.log",
                run_context.run_id, scene_id
            ));
            sinks.push(Box::new(TextTraceSink::create(fallback_path)?));
        }

        let sink: Box<dyn TraceSink> = if sinks.len() == 1 {
            sinks.remove(0)
        } else {
            Box::new(MultiTraceSink::new(sinks))
        };

        let fallback_error_path = run_context.output_dir.join(format!(
            "sim_trace_run_{}_scene_{:04}_logger_errors.log",
            run_context.run_id, scene_id
        ));

        Ok(Self {
            config,
            sink,
            fallback_error_path,
            last_error: None,
        })
    }

    pub fn emit(&mut self, event: SimulationTraceEvent) {
        if let Err(err) = self.sink.write_event(&event) {
            self.last_error = Some(err.to_string());
            self.record_logger_error("write", &err.to_string(), &event.event_name);
            return;
        }

        if self.config.flush_each_event {
            if let Err(err) = self.sink.flush() {
                self.last_error = Some(err.to_string());
                self.record_logger_error("flush", &err.to_string(), &event.event_name);
            }
        }
    }

    pub fn flush(&mut self) {
        if let Err(err) = self.sink.flush() {
            self.last_error = Some(err.to_string());
            self.record_logger_error("flush", &err.to_string(), "sim.logger.flush");
        }
    }

    pub fn output_path(&self) -> PathBuf {
        self.sink.output_path().to_path_buf()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn verbosity(&self) -> SimulationTraceVerbosity {
        self.config.verbosity
    }

    pub fn level(&self) -> SimulationTraceLevel {
        self.config.level
    }

    fn record_logger_error(&mut self, stage: &str, error: &str, original_event: &str) {
        let event = SimulationTraceEvent::new(
            SimulationTraceLevel::Error,
            "sim.logger.write_failed",
            None,
            None,
            None,
            None,
            "failed to append trace log",
            vec![
                ("stage".to_string(), stage.to_string()),
                ("original_event".to_string(), original_event.to_string()),
                ("os_error".to_string(), error.to_string()),
                ("retry".to_string(), "false".to_string()),
            ],
        );

        if let Some(parent) = self.fallback_error_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.fallback_error_path)
        {
            let _ = writeln!(file, "{}", render_text_event(&event));
            let _ = file.flush();
        }
    }
}

pub fn build_run_context(
    output_dir: PathBuf,
    grid_summary: String,
    workfront_count: usize,
    scenario_count: usize,
    upper_floor_threshold: f64,
    lower_floor_completion_ratio: f64,
    lower_floor_forced_completion: usize,
    base_seed: u64,
) -> SimulationTraceRunContext {
    SimulationTraceRunContext {
        run_id: run_id_string(),
        output_dir,
        grid_summary,
        workfront_count,
        scenario_count,
        upper_floor_threshold,
        lower_floor_completion_ratio,
        lower_floor_forced_completion,
        base_seed,
    }
}

fn render_text_event(event: &SimulationTraceEvent) -> String {
    let mut line = format!(
        "{} | {:<7} | {} | scene={} | cycle={} | round={} | wf={} | {}",
        event.timestamp,
        event.level.as_str(),
        event.event_name,
        render_opt_usize(event.scene, 4),
        render_opt_usize(event.cycle, 4),
        render_opt_usize(event.round, 4),
        event.wf
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string()),
        event.message,
    );

    for (key, value) in &event.fields {
        line.push_str(" | ");
        line.push_str(key);
        line.push('=');
        line.push_str(value);
    }
    line
}

fn render_jsonl_event(event: &SimulationTraceEvent) -> String {
    let fields = event
        .fields
        .iter()
        .map(|(key, value)| format!("\"{}\":\"{}\"", escape_json(key), escape_json(value)))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"ts\":\"{}\",\"level\":\"{}\",\"event\":\"{}\",\"scene\":{},\"cycle\":{},\"round\":{},\"wf\":{},\"message\":\"{}\",\"fields\":{{{}}}}}",
        escape_json(&event.timestamp),
        event.level.as_str(),
        escape_json(&event.event_name),
        render_json_opt_usize(event.scene),
        render_json_opt_usize(event.cycle),
        render_json_opt_usize(event.round),
        event.wf
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string()),
        escape_json(&event.message),
        fields,
    )
}

fn render_opt_usize(value: Option<usize>, width: usize) -> String {
    value
        .map(|number| format!("{:0width$}", number, width = width))
        .unwrap_or_else(|| "-".to_string())
}

fn render_json_opt_usize(value: Option<usize>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn escape_json(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn format_ids<T: ToString>(values: impl IntoIterator<Item = T>) -> String {
    let parts = values
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    format!("[{}]", parts.join(","))
}

fn timestamp_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

fn run_id_string() -> String {
    Local::now().format("%Y%m%d_%H%M%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_logger_writes_text_file() {
        let test_dir = std::env::temp_dir().join(format!(
            "assyplan_trace_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        let context = build_run_context(
            test_dir.clone(),
            "2x2x2".to_string(),
            1,
            1,
            0.3,
            0.8,
            10,
            42,
        );
        let config = SimulationTraceConfig {
            enabled: true,
            ..SimulationTraceConfig::default()
        };

        let mut logger = SimulationTraceLogger::create_for_scene(config, &context, 1)
            .expect("trace logger should be created");
        logger.emit(SimulationTraceEvent::new(
            SimulationTraceLevel::Info,
            "sim.run.start",
            Some(1),
            None,
            None,
            None,
            "simulation started",
            vec![("seed".to_string(), "42".to_string())],
        ));
        logger.flush();

        let output_path = logger.output_path();
        let content = std::fs::read_to_string(&output_path)
            .expect("trace log content should be readable");
        assert!(content.contains("sim.run.start"));
        assert!(content.contains("seed=42"));

        let _ = std::fs::remove_file(output_path);
        let _ = std::fs::remove_dir_all(test_dir);
    }
}