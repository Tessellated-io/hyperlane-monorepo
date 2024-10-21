use eyre::Result;
pub use span_metrics::TimeSpanLifetime;
use tracing_subscriber::{
    filter::{filter_fn, LevelFilter, Targets},
    fmt as tracing_fmt,
    prelude::*,
    EnvFilter,
};

use self::fmt::LogOutputLayer;
use crate::{settings::trace::fmt::Style, CoreMetrics};

/// Configure a `tracing_subscriber::fmt` Layer outputting to stdout
pub mod fmt;

mod span_metrics;

/// Logging level. A "higher level" means more will be logged.
#[derive(Default, Debug, Clone, Copy, serde::Deserialize, PartialOrd, Ord, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Off
    Off = 0,
    /// Error
    Error = 1,
    /// Warn
    Warn = 2,
    /// Debug
    Debug = 4,
    /// Trace
    Trace = 5,
    /// Trace + Additional logs from dependencies
    DependencyTrace = 6,
    /// Info
    #[serde(other)]
    #[default]
    Info = 3,
}

impl From<Level> for LevelFilter {
    fn from(level: Level) -> LevelFilter {
        match level {
            Level::Off => LevelFilter::OFF,
            Level::Error => LevelFilter::ERROR,
            Level::Warn => LevelFilter::WARN,
            Level::Debug => LevelFilter::DEBUG,
            Level::Trace | Level::DependencyTrace => LevelFilter::TRACE,
            Level::Info => LevelFilter::INFO,
        }
    }
}

/// Configuration for the tracing subscribers used by Hyperlane agents
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct TracingConfig {
    #[serde(default)]
    pub(crate) fmt: Style,
    #[serde(default)]
    pub(crate) level: Level,
}

impl TracingConfig {
    /// Attempt to instantiate and register a tracing subscriber setup from
    /// settings.
    pub fn start_tracing(&self, metrics: &CoreMetrics) -> Result<console_subscriber::Server> {
        let mut target_layer = Targets::new().with_default(self.level);

        if self.level < Level::DependencyTrace {
            // Reduce log noise from trusted libraries that we can reasonably assume are working correctly
            target_layer = target_layer
                .with_target("hyper::", Level::Info)
                .with_target("rusoto_core", Level::Info)
                .with_target("rustls", Level::Info)
                .with_target("reqwest", Level::Info)
                .with_target("runtime", Level::Debug)
                .with_target("h2::", Level::Info)
                .with_target("tower", Level::Info)
                .with_target("tendermint", Level::Info)
                .with_target("tokio", Level::Debug)
                .with_target("tokio_util", Level::Debug)
                .with_target("ethers_providers", Level::Debug);
        }

        if self.level < Level::Trace {
            // only show sqlx query logs at trace level
            target_layer = target_layer
                .with_target("sqlx::query", Level::Warn)
                .with_target("hyper::", Level::Warn);
        }
        let fmt_layer: LogOutputLayer<_> = self.fmt.into();
        let err_layer = tracing_error::ErrorLayer::default();

        let (tokio_layer, tokio_server) = console_subscriber::ConsoleLayer::new();

        let span_filter = filter_fn(|metadata| {
            // Only log events (no spans) and filter based on log level
            metadata.is_event() || metadata.level() <= &LevelFilter::INFO
        });

        let subscriber = tracing_subscriber::Registry::default()
            .with(EnvFilter::from_default_env()) // Allows filtering levels via `RUST_LOG`
            .with(tokio_layer)
            .with(target_layer)
            .with(TimeSpanLifetime::new(metrics))
            .with(fmt_layer)
            .with(err_layer)
            .with(tracing_fmt::layer().with_span_events(tracing_fmt::format::FmtSpan::NONE))
            .with(tracing_fmt::layer().with_filter(span_filter));

        subscriber.try_init()?;
        Ok(tokio_server)
    }
}
