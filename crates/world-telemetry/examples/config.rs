//! Telemetry configuration example.

use world_telemetry::{derive_traces_endpoint, ExporterProtocol, TelemetryConfig};

fn main() {
    let endpoint =
        derive_traces_endpoint(None, Some("http://otel:4318/"), None).expect("endpoint is derived");
    let config = TelemetryConfig::new("example-service", "info")
        .with_otlp_endpoint(endpoint, ExporterProtocol::HttpProtobuf);
    assert_eq!(config.service_name, "example-service");
}
