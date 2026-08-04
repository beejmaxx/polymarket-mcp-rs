# Contract and release policy

The project uses semantic versions. During `0.x`, a minor release may intentionally change a tool schema; patch releases must remain backward compatible.

The following are public contracts:

- tool names within each profile;
- input and output JSON schemas;
- structured error fields and documented error codes;
- exact decimal and identifier strings;
- CLI flags, environment variables, and exit status from `doctor`;
- SQLite replay semantics documented by the tools.

Every tool has a generated input and output schema. The committed Research and All profile catalogs are golden-tested. A tool removal, rename, required input addition, output type change, semantic unit change, or error-shape change must be called out in the changelog and receive an appropriate version bump.

Adding an optional input or output field, clarifying a description, adding a tool, and improving diagnostics are normally backward compatible. Upstream Polymarket fields may be absent; optionality is preserved instead of inventing values.

Releases include architecture-specific archives, SHA-256 checksums, an SPDX JSON SBOM, GitHub build provenance, and a cross-platform MCP Bundle. Release artifacts are built from the tagged commit in GitHub Actions.
