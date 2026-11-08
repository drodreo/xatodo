//! xa-todo-integration: the superlive shell integration for xatodo.
//!
//! Everything host-specific lives here and nowhere else: the component id
//! (health loop key + registry dispatch key), the owner-attribution
//! semantics (single local owner replacing apikey auth), the component
//! router assembly, the synthesized manifest, and the slsdk event-plane
//! adapter for the core's push handle. The business core
//! (`xa-todo-core`) stays free of all of it — superlive evolution cannot
//! leak into the business crate.

pub mod notifier;
pub mod owner;
pub mod router;
#[cfg(test)]
mod test_support;

/// Component id constant. Note: the shell's dev anti-drift check probes
/// workspace members' `src/main.rs` files for a line-leading
/// `const COMPONENT_ID` — this layout (constant in `integration/src/lib.rs`,
/// referenced by bins via `crate::COMPONENT_ID`) does not match that probe,
/// so the check degrades to warn+skip. Identity is still enforced at
/// runtime: the /health response self-reports this constant and the shell
/// matches it against manifest.id, and the packaging scripts run the same
/// source-of-truth extraction at packaging time. Modules here reference it
/// via `crate::COMPONENT_ID`; it also serves as the registry dispatch key
/// (namespace).
pub const COMPONENT_ID: &str = "xatodo";

use slsdk_rs::{ComponentConfig, ComponentManifest, LaunchConfig, UiDeclaration, UiType};

/// Synthesized manifest: the serve() contract only consumes id / version
/// (in the /health response body); the component process holds no manifest
/// file (the shell keeps it after installation), and the remaining fields are
/// filled with placeholders to satisfy construction.
/// Identity is self-reported by the component and cross-checked by the shell
/// via the health response — consistent with the contract comments in serve.rs.
/// Note: empty files would fail manifest::validate(), but serve() does not
/// call validate (safe at runtime).
pub fn make_manifest() -> ComponentManifest {
    ComponentManifest {
        id: crate::COMPONENT_ID.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        min_shell_version: "0.0.0".to_string(),
        namespace: crate::COMPONENT_ID.to_string(),
        display_name: "xatodo".to_string(),
        ui: Some(UiDeclaration {
            ui_type: UiType::Surface,
            entry: "surface.json".to_string(),
        }),
        menu: None,
        routes: Vec::new(),
        // None = accept all transports the host platform provides (Cargo
        // template default semantics)
        transport: None,
        launch: Some(LaunchConfig {
            command: String::new(),
            args: Vec::new(),
        }),
        files: Vec::new(),
        config: None,
    }
}

/// Connect-form manifest: same synthesized shape as [`make_manifest`] plus
/// the config declaration. The declaration is compiled in from the same file
/// the packaging chain consumes (`scripts/connect-config.json` →
/// gen-manifest.py weaves it into the shipped manifest.json the shell config
/// page renders from) — one source of truth, so the process-side declaration
/// can never drift from the shipped one.
///
/// Why not reuse [`make_manifest`] here: standalone stays config-less (its
/// shipped manifest declares no config section, per the packaging script),
/// and slsdk only mounts `GET/PUT /config` when the manifest declares a
/// config section AND a `ConfigIntegration` is provided — weaving connect's
/// backend fields into the shared manifest would hand standalone a false
/// declaration.
///
/// Parse failures are an `expect`, not a degrade: a broken declaration file
/// silently falling back to `config: None` would resurrect exactly the 404
/// this wiring exists to fix. The file is compile-time pinned and parse-
/// asserted in tests, so an invalid edit fails CI long before any binary
/// ships.
pub fn make_manifest_with_config() -> ComponentManifest {
    let mut manifest = make_manifest();
    let raw = include_str!("../../../scripts/connect-config.json");
    let config: ComponentConfig = serde_json::from_str(raw).expect(
        "bundled connect-config.json (the packaging chain's own source file) must stay valid",
    );
    manifest.config = Some(config);
    manifest
}
