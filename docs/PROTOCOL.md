# Forge event protocol

`forge-protocol` defines stable identifiers, versioned event envelopes, and data payloads shared by runtime adapters and the IDE. Consumers should emit the current JSON envelope when possible. The compatibility adapter also accepts these line-oriented records:

```text
forge_metric:loss=0.42
forge_vector:weights=0.1,0.2,0.3
forge_table:samples={"columns":["x","label"],"rows":[[1,"cat"]]}
```

Framework-neutral ML records currently use `forge_training:`, `forge_evaluation:`, `forge_model:`, `forge_tensor:`, `forge_image:`, `forge_embedding:`, `forge_predictions:`, and `forge_checkpoint:` followed by JSON.

Unknown, malformed, or newer-version envelopes must be rejected without crashing the UI. Large artifacts should be written beneath `.forge/artifacts` and referenced by a safe relative path. Producers must not include credentials, environment secrets, or unrestricted filesystem paths.

Protocol compatibility changes require tests in `crates/forge-protocol` and a documented migration. Additive optional fields are preferred; reinterpreting an existing field is not compatible.
