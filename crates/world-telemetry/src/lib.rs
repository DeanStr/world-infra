//! Shared telemetry setup primitives.

use std::{error::Error, fmt, time::Duration};

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
}

#[cfg(feature = "opentelemetry")]
impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            let _ = provider.shutdown();
        }
    }
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
    global::set_tracer_provider(provider.clone());
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init()
        .map_err(|error| TelemetryError::Subscriber(error.to_string()))?;
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
}
