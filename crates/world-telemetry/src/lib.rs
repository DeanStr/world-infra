//! Shared telemetry setup primitives.

use std::{env, error::Error, fmt, sync::Mutex, time::Duration};

#[cfg(all(
    feature = "opentelemetry",
    any(feature = "otlp-http", feature = "otlp-grpc-tonic")
))]
use opentelemetry::trace::TracerProvider as _;
#[cfg(all(
    feature = "opentelemetry",
    any(feature = "otlp-http", feature = "otlp-grpc-tonic")
))]
use opentelemetry::{global, KeyValue};
#[cfg(all(
    feature = "opentelemetry",
    any(feature = "otlp-http", feature = "otlp-grpc-tonic")
))]
use opentelemetry_otlp::WithExportConfig;
#[cfg(feature = "opentelemetry")]
use opentelemetry_sdk::trace::SdkTracerProvider;
#[cfg(all(
    feature = "opentelemetry",
    any(feature = "otlp-http", feature = "otlp-grpc-tonic")
))]
use opentelemetry_sdk::Resource;
#[cfg(feature = "subscriber")]
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// OTLP exporter protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExporterProtocol {
    /// HTTP/protobuf OTLP traces.
    HttpProtobuf,
    /// gRPC/tonic OTLP traces.
    GrpcTonic,
}

/// Startup failure behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureMode {
    /// Return an error when exporter/subscriber setup fails.
    Strict,
    /// Fall back to local tracing when exporter setup fails.
    BestEffort,
}

/// Telemetry configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryConfig {
    /// Service name.
    pub service_name: String,
    /// Default tracing filter.
    pub default_filter: String,
    /// Optional OTLP traces endpoint.
    pub otlp_endpoint: Option<String>,
    /// Exporter protocol.
    pub exporter_protocol: ExporterProtocol,
    /// Service version resource metadata.
    pub service_version: Option<String>,
    /// Deployment environment resource metadata.
    pub deployment_environment: Option<String>,
    /// Exporter timeout.
    pub timeout: Duration,
    /// Failure behavior.
    pub failure_mode: FailureMode,
}

impl TelemetryConfig {
    /// Construct a config with safe defaults.
    #[must_use]
    pub fn new(service_name: impl Into<String>, default_filter: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            default_filter: default_filter.into(),
            otlp_endpoint: None,
            exporter_protocol: ExporterProtocol::HttpProtobuf,
            service_version: None,
            deployment_environment: None,
            timeout: Duration::from_secs(10),
            failure_mode: FailureMode::Strict,
        }
    }

    /// Set OTLP endpoint and protocol.
    #[must_use]
    pub fn with_otlp_endpoint(
        mut self,
        endpoint: impl Into<String>,
        protocol: ExporterProtocol,
    ) -> Self {
        self.otlp_endpoint = Some(endpoint.into());
        self.exporter_protocol = protocol;
        self
    }

    /// Construct a config from common OpenTelemetry environment variables.
    ///
    /// Product-specific deployment policy remains outside this helper. It reads
    /// `OTEL_SERVICE_NAME`, `OTEL_SERVICE_VERSION`,
    /// `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`, `OTEL_EXPORTER_OTLP_ENDPOINT`,
    /// `OTEL_EXPORTER_OTLP_TRACES_PROTOCOL`, `OTEL_EXPORTER_OTLP_PROTOCOL`,
    /// `OTEL_DEPLOYMENT_ENVIRONMENT`, `DEPLOYMENT_ENVIRONMENT`, and
    /// `OTEL_RESOURCE_ATTRIBUTES`.
    #[must_use]
    pub fn from_standard_env(
        service_name: impl Into<String>,
        default_filter: impl Into<String>,
    ) -> Self {
        Self::from_standard_env_with_options(
            service_name,
            default_filter,
            TelemetryEnvOptions::default(),
        )
    }

    /// Construct a config from common OpenTelemetry environment variables with
    /// explicit fallback options.
    #[must_use]
    pub fn from_standard_env_with_options(
        service_name: impl Into<String>,
        default_filter: impl Into<String>,
        options: TelemetryEnvOptions,
    ) -> Self {
        let service_name = env_value("OTEL_SERVICE_NAME").unwrap_or_else(|| service_name.into());
        let traces_endpoint = env_value("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT");
        let generic_endpoint = env_value("OTEL_EXPORTER_OTLP_ENDPOINT");
        let protocol = env_value("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL")
            .or_else(|| env_value("OTEL_EXPORTER_OTLP_PROTOCOL"))
            .as_deref()
            .map(parse_exporter_protocol)
            .unwrap_or(options.default_protocol);
        let deployment_environment = env_value("OTEL_DEPLOYMENT_ENVIRONMENT")
            .or_else(|| env_value("DEPLOYMENT_ENVIRONMENT"))
            .or_else(|| resource_attribute("deployment.environment.name"))
            .or_else(|| resource_attribute("deployment.environment"));

        Self {
            service_name,
            default_filter: default_filter.into(),
            otlp_endpoint: derive_standard_env_endpoint(
                traces_endpoint.as_deref(),
                generic_endpoint.as_deref(),
                protocol,
                options.append_http_traces_path,
            ),
            exporter_protocol: protocol,
            service_version: env_value("OTEL_SERVICE_VERSION"),
            deployment_environment,
            timeout: Duration::from_secs(10),
            failure_mode: FailureMode::Strict,
        }
    }
}

/// Options for [`TelemetryConfig::from_standard_env_with_options`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelemetryEnvOptions {
    /// Protocol to use when `OTEL_EXPORTER_OTLP_PROTOCOL` is absent.
    pub default_protocol: ExporterProtocol,
    /// Whether generic HTTP OTLP endpoints should receive `/v1/traces`.
    pub append_http_traces_path: bool,
}

impl Default for TelemetryEnvOptions {
    fn default() -> Self {
        Self {
            default_protocol: ExporterProtocol::HttpProtobuf,
            append_http_traces_path: true,
        }
    }
}

fn env_value(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_exporter_protocol(value: &str) -> ExporterProtocol {
    match value.trim().to_ascii_lowercase().as_str() {
        "grpc" | "grpc-tonic" => ExporterProtocol::GrpcTonic,
        _ => ExporterProtocol::HttpProtobuf,
    }
}

fn resource_attribute(name: &str) -> Option<String> {
    env_value("OTEL_RESOURCE_ATTRIBUTES").and_then(|attributes| {
        attributes.split(',').find_map(|attribute| {
            let (key, value) = attribute.split_once('=')?;
            (key.trim() == name)
                .then(|| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
    })
}

fn derive_standard_env_endpoint(
    traces_endpoint: Option<&str>,
    generic_endpoint: Option<&str>,
    protocol: ExporterProtocol,
    append_http_traces_path: bool,
) -> Option<String> {
    traces_endpoint
        .and_then(non_blank)
        .map(ToOwned::to_owned)
        .or_else(|| {
            generic_endpoint.and_then(non_blank).map(|endpoint| {
                if protocol == ExporterProtocol::HttpProtobuf && append_http_traces_path {
                    format!("{}/v1/traces", endpoint.trim_end_matches('/'))
                } else {
                    endpoint.to_owned()
                }
            })
        })
}

/// Derive an OTLP traces endpoint using common `/v1/traces` suffixing rules.
#[must_use]
pub fn derive_traces_endpoint(
    traces_endpoint: Option<&str>,
    product_endpoint: Option<&str>,
    generic_endpoint: Option<&str>,
) -> Option<String> {
    traces_endpoint
        .and_then(non_blank)
        .map(ToOwned::to_owned)
        .or_else(|| {
            product_endpoint
                .and_then(non_blank)
                .or_else(|| generic_endpoint.and_then(non_blank))
                .map(|endpoint| format!("{}/v1/traces", endpoint.trim_end_matches('/')))
        })
}

fn non_blank(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

/// Telemetry setup error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryError {
    /// Subscriber initialization failed.
    Subscriber(String),
    /// Exporter initialization failed.
    Exporter(String),
    /// Feature required by config is disabled.
    FeatureDisabled(&'static str),
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Subscriber(error) => {
                write!(f, "failed to initialize tracing subscriber: {error}")
            }
            Self::Exporter(error) => write!(f, "failed to initialize OTLP exporter: {error}"),
            Self::FeatureDisabled(feature) => write!(f, "telemetry feature {feature} is disabled"),
        }
    }
}

impl Error for TelemetryError {}

/// Error returned when telemetry shutdown fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryShutdownError {
    message: String,
}

impl TelemetryShutdownError {
    /// Create a shutdown error from an underlying provider error message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TelemetryShutdownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to shut down OpenTelemetry tracer provider: {}",
            self.message
        )
    }
}

impl Error for TelemetryShutdownError {}

/// Guard that flushes telemetry on drop when an OpenTelemetry provider exists.
#[derive(Debug)]
pub struct TelemetryGuard {
    #[cfg(feature = "opentelemetry")]
    provider: Option<SdkTracerProvider>,
}

impl TelemetryGuard {
    #[cfg(feature = "subscriber")]
    fn local() -> Self {
        Self {
            #[cfg(feature = "opentelemetry")]
            provider: None,
        }
    }

    #[cfg(all(
        feature = "opentelemetry",
        any(feature = "otlp-http", feature = "otlp-grpc-tonic")
    ))]
    fn with_provider(provider: SdkTracerProvider) -> Self {
        Self {
            provider: Some(provider),
        }
    }

    /// Flush and shut down the OpenTelemetry tracer provider, if one exists.
    ///
    /// Calling this explicitly lets services and tests observe shutdown errors.
    /// Dropping the guard still performs best-effort shutdown as a fallback.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryShutdownError`] when the OpenTelemetry provider
    /// reports a shutdown failure.
    pub fn shutdown(&mut self) -> Result<(), TelemetryShutdownError> {
        #[cfg(feature = "opentelemetry")]
        if let Some(provider) = self.provider.take() {
            provider
                .shutdown()
                .map_err(|error| TelemetryShutdownError::new(error.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("{error}");
        }
    }
}

/// Process-local one-time telemetry initializer.
///
/// This is intended for binary/service entrypoints and tests that need a
/// best-effort "initialize once" primitive. Libraries should accept tracing as
/// process-owned infrastructure instead of calling this internally.
#[derive(Debug)]
pub struct TelemetryOnce {
    state: Mutex<TelemetryOnceState>,
}

#[derive(Debug)]
enum TelemetryOnceState {
    Empty,
    Active(TelemetryGuard),
    Shutdown,
}

impl TelemetryOnce {
    /// Create an empty one-time initializer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(TelemetryOnceState::Empty),
        }
    }

    /// Initialize telemetry once.
    ///
    /// Returns `Ok(true)` when this call initialized telemetry and `Ok(false)`
    /// when telemetry was already initialized through this guard or this guard
    /// was previously shut down. Shutdown is terminal because the global
    /// tracing subscriber cannot be unset.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] from [`init_telemetry`].
    pub fn init(&self, config: &TelemetryConfig) -> Result<bool, TelemetryError> {
        let mut state = self
            .state
            .lock()
            .map_err(|error| TelemetryError::Subscriber(error.to_string()))?;
        if !matches!(*state, TelemetryOnceState::Empty) {
            return Ok(false);
        }
        *state = TelemetryOnceState::Active(init_telemetry(config)?);
        Ok(true)
    }

    /// Shut down the guard initialized through this instance, if any.
    ///
    /// After shutdown, later [`TelemetryOnce::init`] calls return `Ok(false)`
    /// instead of attempting to reinstall the process-global tracing subscriber.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryShutdownError`] if provider shutdown fails.
    pub fn shutdown(&self) -> Result<(), TelemetryShutdownError> {
        let mut state = self
            .state
            .lock()
            .map_err(|error| TelemetryShutdownError::new(error.to_string()))?;
        let current = std::mem::replace(&mut *state, TelemetryOnceState::Shutdown);
        if let TelemetryOnceState::Active(mut guard) = current {
            guard.shutdown()?;
        }
        Ok(())
    }
}

impl Default for TelemetryOnce {
    fn default() -> Self {
        Self::new()
    }
}

static GLOBAL_TELEMETRY_ONCE: TelemetryOnce = TelemetryOnce::new();

/// Initialize global process telemetry once.
///
/// See [`TelemetryOnce::init`] for semantics.
///
/// # Errors
///
/// Returns [`TelemetryError`] from [`init_telemetry`].
pub fn init_global_once(config: &TelemetryConfig) -> Result<bool, TelemetryError> {
    GLOBAL_TELEMETRY_ONCE.init(config)
}

/// Shut down telemetry initialized with [`init_global_once`].
///
/// # Errors
///
/// Returns [`TelemetryShutdownError`] if provider shutdown fails.
pub fn shutdown_global_once() -> Result<(), TelemetryShutdownError> {
    GLOBAL_TELEMETRY_ONCE.shutdown()
}

/// Initialize tracing from a [`TelemetryConfig`].
///
/// # Errors
///
/// Returns [`TelemetryError`] when subscriber or exporter setup fails and the
/// configured failure mode is strict.
#[cfg(feature = "subscriber")]
pub fn init_telemetry(config: &TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    match init_telemetry_inner(config) {
        Ok(guard) => Ok(guard),
        Err(_error) if config.failure_mode == FailureMode::BestEffort => {
            init_local_subscriber(&config.default_filter)?;
            Ok(TelemetryGuard::local())
        }
        Err(error) => Err(error),
    }
}

#[cfg(feature = "subscriber")]
fn init_telemetry_inner(config: &TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.default_filter.clone()));

    #[cfg(feature = "opentelemetry")]
    if let Some(endpoint) = config.otlp_endpoint.as_deref() {
        #[cfg(any(feature = "otlp-http", feature = "otlp-grpc-tonic"))]
        return init_otel_subscriber(config, endpoint, filter);

        #[cfg(not(any(feature = "otlp-http", feature = "otlp-grpc-tonic")))]
        {
            let _ = endpoint;
            return Err(TelemetryError::FeatureDisabled(
                "otlp-http or otlp-grpc-tonic",
            ));
        }
    }

    #[cfg(not(feature = "opentelemetry"))]
    if config.otlp_endpoint.is_some() {
        return Err(TelemetryError::FeatureDisabled("opentelemetry"));
    }

    init_local_subscriber_with_filter(filter)?;
    Ok(TelemetryGuard::local())
}

#[cfg(feature = "subscriber")]
fn init_local_subscriber(default_filter: &str) -> Result<(), TelemetryError> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    init_local_subscriber_with_filter(filter)
}

#[cfg(feature = "subscriber")]
fn init_local_subscriber_with_filter(filter: EnvFilter) -> Result<(), TelemetryError> {
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .try_init()
        .map_err(|error| TelemetryError::Subscriber(error.to_string()))
}

#[cfg(all(
    feature = "subscriber",
    feature = "opentelemetry",
    any(feature = "otlp-http", feature = "otlp-grpc-tonic")
))]
fn init_otel_subscriber(
    config: &TelemetryConfig,
    endpoint: &str,
    filter: EnvFilter,
) -> Result<TelemetryGuard, TelemetryError> {
    let exporter = match config.exporter_protocol {
        ExporterProtocol::HttpProtobuf => build_http_exporter(endpoint, config.timeout)?,
        ExporterProtocol::GrpcTonic => build_grpc_exporter(endpoint, config.timeout)?,
    };
    let provider = SdkTracerProvider::builder()
        .with_resource(resource(config))
        .with_batch_exporter(exporter)
        .build();
    let tracer = provider.tracer(config.service_name.clone());
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init()
        .map_err(|error| TelemetryError::Subscriber(error.to_string()))?;
    global::set_tracer_provider(provider.clone());
    Ok(TelemetryGuard::with_provider(provider))
}

#[cfg(all(feature = "opentelemetry", feature = "otlp-http"))]
fn build_http_exporter(
    endpoint: &str,
    timeout: Duration,
) -> Result<opentelemetry_otlp::SpanExporter, TelemetryError> {
    opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint.to_owned())
        .with_timeout(timeout)
        .build()
        .map_err(|error| TelemetryError::Exporter(error.to_string()))
}

#[cfg(all(
    feature = "opentelemetry",
    any(feature = "otlp-http", feature = "otlp-grpc-tonic"),
    not(feature = "otlp-http")
))]
fn build_http_exporter(
    _endpoint: &str,
    _timeout: Duration,
) -> Result<opentelemetry_otlp::SpanExporter, TelemetryError> {
    Err(TelemetryError::FeatureDisabled("otlp-http"))
}

#[cfg(all(feature = "opentelemetry", feature = "otlp-grpc-tonic"))]
fn build_grpc_exporter(
    endpoint: &str,
    timeout: Duration,
) -> Result<opentelemetry_otlp::SpanExporter, TelemetryError> {
    opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.to_owned())
        .with_timeout(timeout)
        .build()
        .map_err(|error| TelemetryError::Exporter(error.to_string()))
}

#[cfg(all(
    feature = "opentelemetry",
    any(feature = "otlp-http", feature = "otlp-grpc-tonic"),
    not(feature = "otlp-grpc-tonic")
))]
fn build_grpc_exporter(
    _endpoint: &str,
    _timeout: Duration,
) -> Result<opentelemetry_otlp::SpanExporter, TelemetryError> {
    Err(TelemetryError::FeatureDisabled("otlp-grpc-tonic"))
}

#[cfg(all(
    feature = "opentelemetry",
    any(feature = "otlp-http", feature = "otlp-grpc-tonic")
))]
fn resource(config: &TelemetryConfig) -> Resource {
    let mut builder = Resource::builder().with_service_name(config.service_name.clone());
    let mut attributes = Vec::new();
    if let Some(version) = &config.service_version {
        attributes.push(KeyValue::new("service.version", version.clone()));
    }
    if let Some(environment) = &config.deployment_environment {
        attributes.push(KeyValue::new(
            "deployment.environment.name",
            environment.clone(),
        ));
    }
    if !attributes.is_empty() {
        builder = builder.with_attributes(attributes);
    }
    builder.build()
}

/// Initialize local tracing without subscriber support.
///
/// # Errors
///
/// Always returns [`TelemetryError::FeatureDisabled`].
#[cfg(not(feature = "subscriber"))]
pub fn init_telemetry(_config: &TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    Err(TelemetryError::FeatureDisabled("subscriber"))
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn endpoint_derivation_preserves_traces_endpoint() {
        assert_eq!(
            derive_traces_endpoint(Some("http://otel/v1/traces"), Some("http://product"), None),
            Some("http://otel/v1/traces".to_owned())
        );
    }

    #[test]
    fn endpoint_derivation_suffixes_generic_endpoint() {
        assert_eq!(
            derive_traces_endpoint(None, Some("http://otel/"), None),
            Some("http://otel/v1/traces".to_owned())
        );
    }

    #[test]
    fn config_defaults_are_strict_http() {
        let config = TelemetryConfig::new("svc", "info");
        assert_eq!(config.exporter_protocol, ExporterProtocol::HttpProtobuf);
        assert_eq!(config.failure_mode, FailureMode::Strict);
    }

    #[test]
    fn standard_env_config_reads_common_otlp_values() {
        let _guard = ENV_LOCK.lock().expect("env lock should not be poisoned");
        let previous_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
        let previous_protocol = env::var("OTEL_EXPORTER_OTLP_PROTOCOL").ok();
        let previous_traces_protocol = env::var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL").ok();
        let previous_traces = env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").ok();
        let previous_environment = env::var("OTEL_DEPLOYMENT_ENVIRONMENT").ok();
        env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://otel");
        env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf");
        env::remove_var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL");
        env::remove_var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT");
        env::set_var("OTEL_DEPLOYMENT_ENVIRONMENT", "staging");

        let config = TelemetryConfig::from_standard_env("svc", "info");
        assert_eq!(
            config.otlp_endpoint.as_deref(),
            Some("http://otel/v1/traces")
        );
        assert_eq!(config.exporter_protocol, ExporterProtocol::HttpProtobuf);
        assert_eq!(config.deployment_environment.as_deref(), Some("staging"));

        restore_env("OTEL_EXPORTER_OTLP_ENDPOINT", previous_endpoint);
        restore_env("OTEL_EXPORTER_OTLP_PROTOCOL", previous_protocol);
        restore_env(
            "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
            previous_traces_protocol,
        );
        restore_env("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", previous_traces);
        restore_env("OTEL_DEPLOYMENT_ENVIRONMENT", previous_environment);
    }

    #[test]
    fn standard_env_config_keeps_grpc_generic_endpoint_raw() {
        let _guard = ENV_LOCK.lock().expect("env lock should not be poisoned");
        let previous_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
        let previous_protocol = env::var("OTEL_EXPORTER_OTLP_PROTOCOL").ok();
        let previous_traces_protocol = env::var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL").ok();
        let previous_traces = env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").ok();
        env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://otel:4317");
        env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");
        env::remove_var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL");
        env::remove_var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT");

        let config = TelemetryConfig::from_standard_env("svc", "info");
        assert_eq!(config.otlp_endpoint.as_deref(), Some("http://otel:4317"));
        assert_eq!(config.exporter_protocol, ExporterProtocol::GrpcTonic);

        restore_env("OTEL_EXPORTER_OTLP_ENDPOINT", previous_endpoint);
        restore_env("OTEL_EXPORTER_OTLP_PROTOCOL", previous_protocol);
        restore_env(
            "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
            previous_traces_protocol,
        );
        restore_env("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", previous_traces);
    }

    #[test]
    fn traces_protocol_env_overrides_generic_protocol() {
        let _guard = ENV_LOCK.lock().expect("env lock should not be poisoned");
        let previous_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
        let previous_protocol = env::var("OTEL_EXPORTER_OTLP_PROTOCOL").ok();
        let previous_traces_protocol = env::var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL").ok();
        let previous_traces = env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").ok();
        env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://otel:4317");
        env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf");
        env::set_var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "grpc");
        env::remove_var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT");

        let config = TelemetryConfig::from_standard_env("svc", "info");
        assert_eq!(config.otlp_endpoint.as_deref(), Some("http://otel:4317"));
        assert_eq!(config.exporter_protocol, ExporterProtocol::GrpcTonic);

        restore_env("OTEL_EXPORTER_OTLP_ENDPOINT", previous_endpoint);
        restore_env("OTEL_EXPORTER_OTLP_PROTOCOL", previous_protocol);
        restore_env(
            "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
            previous_traces_protocol,
        );
        restore_env("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", previous_traces);
    }

    #[test]
    fn traces_endpoint_env_is_exact_even_for_grpc() {
        let _guard = ENV_LOCK.lock().expect("env lock should not be poisoned");
        let previous_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
        let previous_protocol = env::var("OTEL_EXPORTER_OTLP_PROTOCOL").ok();
        let previous_traces_protocol = env::var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL").ok();
        let previous_traces = env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").ok();
        env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://otel:4317");
        env::set_var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", "http://traces:4317");
        env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");
        env::remove_var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL");

        let config = TelemetryConfig::from_standard_env("svc", "info");
        assert_eq!(config.otlp_endpoint.as_deref(), Some("http://traces:4317"));

        restore_env("OTEL_EXPORTER_OTLP_ENDPOINT", previous_endpoint);
        restore_env("OTEL_EXPORTER_OTLP_PROTOCOL", previous_protocol);
        restore_env(
            "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
            previous_traces_protocol,
        );
        restore_env("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", previous_traces);
    }

    #[test]
    fn shutdown_error_display_names_shutdown() {
        assert_eq!(
            TelemetryShutdownError::new("boom").to_string(),
            "failed to shut down OpenTelemetry tracer provider: boom"
        );
    }

    #[test]
    #[cfg(feature = "subscriber")]
    fn local_guard_shutdown_is_ok_and_idempotent() {
        let mut guard = TelemetryGuard::local();
        guard.shutdown().expect("local guard shutdown should be ok");
        guard
            .shutdown()
            .expect("second local guard shutdown should be ok");
    }

    #[test]
    fn telemetry_once_shutdown_before_init_is_terminal() {
        let once = TelemetryOnce::new();
        once.shutdown().expect("empty shutdown should be ok");
        assert_eq!(
            once.init(&TelemetryConfig::new("svc", "info")),
            Ok(false),
            "shutdown should prevent later subscriber initialization"
        );
    }

    #[test]
    #[cfg(feature = "subscriber")]
    fn telemetry_once_shutdown_after_active_guard_is_terminal() {
        let once = TelemetryOnce {
            state: Mutex::new(TelemetryOnceState::Active(TelemetryGuard::local())),
        };
        once.shutdown().expect("active shutdown should be ok");
        assert_eq!(
            once.init(&TelemetryConfig::new("svc", "info")),
            Ok(false),
            "shutdown should make later init a no-op"
        );
        once.shutdown()
            .expect("second shutdown after terminal state should be ok");
    }

    fn restore_env(name: &str, value: Option<String>) {
        if let Some(value) = value {
            env::set_var(name, value);
        } else {
            env::remove_var(name);
        }
    }
}
