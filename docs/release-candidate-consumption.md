# Release-Candidate Product Consumption

Path dependencies are allowed only for local extraction spikes. A product branch
that is intended to be committed must consume shared crates from an immutable git
tag or an exact revision.

Generate exact-revision dependency entries with:

```sh
scripts/pinned-product-deps.sh
```

The helper requires a real `world-infra` commit. Before the first commit exists,
it exits with an error so local path dependencies are not accidentally treated as
release-candidate evidence.

For local release-candidate testing without a configured remote, the helper emits
a `file://` git URL pinned to the exact commit. For production branches, pass the
canonical repository URL explicitly:

```sh
scripts/pinned-product-deps.sh \
  --url ssh://git@github.com/OWNER/world-infra.git \
  --rev <release-candidate-commit>
```

After patching a product to the emitted entries, run the consumer canary commands
recorded in `docs/consumer-canaries/` and update the canary record with the exact
revision, command output, failures, and disposition.

The generated snippets include product-specific feature selections. In
particular, Chairman consumes `world-telemetry` with `otlp-http`, while Airline
consumes it with `otlp-grpc-tonic`.
