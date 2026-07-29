//! Approved templates.
//!
//! Version one has exactly three. There is no free-form image field, no
//! user-supplied Dockerfile, and no registry pull of an arbitrary reference.
//! Adding a template is a code change with review, not a user action.
//!
//! The manifest declares what a project may ask for; [`TemplateRegistry::validate`]
//! refuses anything outside it *before* rendering. That is what turns
//! "choose an install command" into picking from a list rather than typing a
//! shell line.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("no approved template named `{0}`")]
    UnknownTemplate(String),
    #[error("`{version}` is not a supported {template} version (supported: {supported})")]
    UnsupportedVersion {
        template: String,
        version: String,
        supported: String,
    },
    #[error("`{manager}` is not a valid package manager for {template}")]
    UnsupportedPackageManager { template: String, manager: String },
    #[error("`{kind}` health checks are not supported by {template}")]
    UnsupportedHealthCheck { template: String, kind: String },
    #[error("could not read the template manifest at {path}: {detail}")]
    Unreadable { path: String, detail: String },
    #[error("the template manifest at {path} is malformed: {detail}")]
    Malformed { path: String, detail: String },
    #[error("template `{0}` declares no default version")]
    NoDefaultVersion(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateVersion {
    pub version: String,
    pub base_image: String,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub lts: bool,
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCheckSupport {
    pub supported: Vec<String>,
    pub default: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateManifest {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub versions: Vec<TemplateVersion>,
    #[serde(default)]
    pub build_versions: Vec<TemplateVersion>,
    pub package_managers: Vec<String>,
    pub default_package_manager: String,
    /// Allow-listed argument vectors, keyed by package manager.
    #[serde(default)]
    pub install_commands: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub build_commands: BTreeMap<String, Vec<String>>,
    pub start_forms: Vec<String>,
    #[serde(default)]
    pub development_scripts: Vec<String>,
    pub working_dir: String,
    pub default_port: u16,
    pub health_check: HealthCheckSupport,
}

impl TemplateManifest {
    pub fn default_version(&self) -> Option<&TemplateVersion> {
        self.versions
            .iter()
            .find(|version| version.default)
            .or_else(|| self.versions.first())
    }

    pub fn find_version(&self, version: &str) -> Option<&TemplateVersion> {
        self.versions.iter().find(|entry| entry.version == version)
    }

    pub fn supported_versions(&self) -> String {
        self.versions
            .iter()
            .map(|entry| entry.version.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The argument vector for an install, or `None` when the manager needs no
    /// install step. Never a string, so there is nothing for a shell to parse.
    pub fn install_command(&self, manager: &str, has_lockfile: bool) -> Option<Vec<String>> {
        // The no-lockfile fallback is a separate declared entry rather than an
        // improvised edit of the frozen one.
        if !has_lockfile {
            let fallback = format!("{manager}_NO_LOCKFILE");
            if let Some(command) = self.install_commands.get(&fallback) {
                return Some(command.clone());
            }
        }
        self.install_commands.get(manager).cloned()
    }

    pub fn build_command(&self, manager: &str) -> Option<Vec<String>> {
        self.build_commands.get(manager).cloned()
    }

    pub fn is_development_script(&self, script: &str) -> bool {
        self.development_scripts.iter().any(|entry| entry == script)
    }
}

/// What a project asked for. Validated against a manifest before use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRequest {
    pub template_id: String,
    pub version: String,
    pub package_manager: String,
    pub health_check_kind: String,
}

/// Every approved template, loaded once at startup.
#[derive(Debug, Clone, Default)]
pub struct TemplateRegistry {
    templates: BTreeMap<String, TemplateManifest>,
}

impl TemplateRegistry {
    /// Load every `manifest.toml` under a templates directory.
    ///
    /// Only the three known ids are accepted. A stray directory appearing there
    /// — dropped in by a user, or left by a bad upgrade — is ignored rather
    /// than silently becoming a usable template.
    pub fn load(templates_dir: &Path) -> Result<Self, TemplateError> {
        const APPROVED: &[&str] = &["nodejs", "python", "static-site"];

        let mut templates = BTreeMap::new();
        for id in APPROVED {
            let path = templates_dir.join(id).join("manifest.toml");
            let raw =
                std::fs::read_to_string(&path).map_err(|error| TemplateError::Unreadable {
                    path: path.display().to_string(),
                    detail: error.to_string(),
                })?;
            let manifest: TemplateManifest =
                toml::from_str(&raw).map_err(|error| TemplateError::Malformed {
                    path: path.display().to_string(),
                    detail: error.to_string(),
                })?;

            if manifest.id != *id {
                return Err(TemplateError::Malformed {
                    path: path.display().to_string(),
                    detail: format!("declares id `{}` but lives in `{id}`", manifest.id),
                });
            }
            if manifest.default_version().is_none() {
                return Err(TemplateError::NoDefaultVersion(manifest.id.clone()));
            }

            templates.insert(manifest.id.clone(), manifest);
        }

        Ok(Self { templates })
    }

    /// Build a registry from already-parsed manifests. For tests.
    pub fn from_manifests(manifests: Vec<TemplateManifest>) -> Self {
        Self {
            templates: manifests
                .into_iter()
                .map(|manifest| (manifest.id.clone(), manifest))
                .collect(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&TemplateManifest> {
        self.templates.get(id)
    }

    pub fn ids(&self) -> Vec<&str> {
        self.templates.keys().map(String::as_str).collect()
    }

    /// Refuse anything the manifest does not permit.
    ///
    /// Called at the API boundary, so an out-of-range value never reaches a
    /// render, a build, or a container.
    pub fn validate(&self, request: &RuntimeRequest) -> Result<&TemplateManifest, TemplateError> {
        let manifest = self
            .templates
            .get(&request.template_id)
            .ok_or_else(|| TemplateError::UnknownTemplate(request.template_id.clone()))?;

        if manifest.find_version(&request.version).is_none() {
            return Err(TemplateError::UnsupportedVersion {
                template: manifest.id.clone(),
                version: request.version.clone(),
                supported: manifest.supported_versions(),
            });
        }

        if !manifest
            .package_managers
            .iter()
            .any(|manager| manager == &request.package_manager)
        {
            return Err(TemplateError::UnsupportedPackageManager {
                template: manifest.id.clone(),
                manager: request.package_manager.clone(),
            });
        }

        if !manifest
            .health_check
            .supported
            .iter()
            .any(|kind| kind == &request.health_check_kind)
        {
            return Err(TemplateError::UnsupportedHealthCheck {
                template: manifest.id.clone(),
                kind: request.health_check_kind.clone(),
            });
        }

        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real templates shipped with the product.
    fn registry() -> TemplateRegistry {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("docker")
            .join("templates");
        TemplateRegistry::load(&root).expect("the shipped templates must load")
    }

    #[test]
    fn the_three_shipped_templates_load() {
        let registry = registry();
        let mut ids = registry.ids();
        ids.sort_unstable();
        assert_eq!(ids, vec!["nodejs", "python", "static-site"]);
    }

    #[test]
    fn every_template_declares_a_default_version() {
        for id in registry().ids() {
            let manifest = registry().get(id).expect("manifest").clone();
            assert!(
                manifest.default_version().is_some(),
                "{id} has no default version"
            );
        }
    }

    #[test]
    fn a_valid_request_passes() {
        let request = RuntimeRequest {
            template_id: "nodejs".to_string(),
            version: "22".to_string(),
            package_manager: "PNPM".to_string(),
            health_check_kind: "NONE".to_string(),
        };
        assert!(registry().validate(&request).is_ok());
    }

    #[test]
    fn an_unknown_template_is_refused() {
        let request = RuntimeRequest {
            template_id: "my-custom-image".to_string(),
            version: "22".to_string(),
            package_manager: "PNPM".to_string(),
            health_check_kind: "NONE".to_string(),
        };
        assert!(matches!(
            registry().validate(&request),
            Err(TemplateError::UnknownTemplate(_))
        ));
    }

    #[test]
    fn an_unsupported_version_is_refused_and_lists_what_is_supported() {
        let request = RuntimeRequest {
            template_id: "nodejs".to_string(),
            version: "10".to_string(),
            package_manager: "PNPM".to_string(),
            health_check_kind: "NONE".to_string(),
        };
        let error = registry().validate(&request).expect_err("should refuse");
        match error {
            TemplateError::UnsupportedVersion { supported, .. } => {
                assert!(supported.contains("22"), "{supported}");
            }
            other => panic!("wrong error: {other}"),
        }
    }

    #[test]
    fn a_package_manager_from_another_runtime_is_refused() {
        // POETRY is a Python manager; asking for it on Node must fail.
        let request = RuntimeRequest {
            template_id: "nodejs".to_string(),
            version: "22".to_string(),
            package_manager: "POETRY".to_string(),
            health_check_kind: "NONE".to_string(),
        };
        assert!(matches!(
            registry().validate(&request),
            Err(TemplateError::UnsupportedPackageManager { .. })
        ));
    }

    #[test]
    fn an_unsupported_health_check_is_refused() {
        let request = RuntimeRequest {
            template_id: "nodejs".to_string(),
            version: "22".to_string(),
            package_manager: "PNPM".to_string(),
            health_check_kind: "COMMAND".to_string(),
        };
        assert!(matches!(
            registry().validate(&request),
            Err(TemplateError::UnsupportedHealthCheck { .. })
        ));
    }

    #[test]
    fn install_commands_are_argument_vectors_not_shell_strings() {
        let registry = registry();
        let node = registry.get("nodejs").expect("nodejs");
        let command = node.install_command("PNPM", true).expect("command");
        assert_eq!(
            command,
            vec![
                "pnpm".to_string(),
                "install".to_string(),
                "--frozen-lockfile".to_string()
            ]
        );
        // Nothing a shell would interpret.
        for part in &command {
            assert!(!part.contains(';'), "{part}");
            assert!(!part.contains("&&"), "{part}");
            assert!(!part.contains('|'), "{part}");
        }
    }

    #[test]
    fn the_no_lockfile_fallback_is_declared_not_improvised() {
        let registry = registry();
        let node = registry.get("nodejs").expect("nodejs");

        let frozen = node.install_command("PNPM", true).expect("frozen");
        let fallback = node.install_command("PNPM", false).expect("fallback");

        assert!(frozen.contains(&"--frozen-lockfile".to_string()));
        assert!(
            !fallback.contains(&"--frozen-lockfile".to_string()),
            "the fallback must drop the frozen flag: {fallback:?}"
        );
    }

    #[test]
    fn python_has_no_lockfile_fallback_because_pip_needs_none() {
        let registry = registry();
        let python = registry.get("python").expect("python");
        // pip install -r requirements.txt works with or without pins.
        assert_eq!(
            python.install_command("PIP", true),
            python.install_command("PIP", false)
        );
    }

    #[test]
    fn development_scripts_are_declared_per_template() {
        let registry = registry();
        let node = registry.get("nodejs").expect("nodejs");
        assert!(node.is_development_script("dev"));
        assert!(node.is_development_script("nodemon"));
        assert!(!node.is_development_script("start"));
    }

    #[test]
    fn base_images_are_pinned_to_a_patch_version() {
        // A floating tag would let a republished image change what runs.
        for id in ["nodejs", "python", "static-site"] {
            let registry = registry();
            let manifest = registry.get(id).expect("manifest");
            for version in &manifest.versions {
                let tag = version.base_image.rsplit(':').next().unwrap_or("");
                assert!(
                    tag.matches('.').count() >= 1,
                    "{id} version {} uses a floating tag: {}",
                    version.version,
                    version.base_image
                );
            }
        }
    }

    #[test]
    fn a_directory_that_is_not_an_approved_template_is_ignored() {
        // Dropping a folder into docker/templates must not create a template.
        let directory = tempfile::tempdir().expect("temp dir");
        let root = directory.path();

        for id in ["nodejs", "python", "static-site"] {
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("docker")
                .join("templates")
                .join(id)
                .join("manifest.toml");
            let target = root.join(id);
            std::fs::create_dir_all(&target).expect("create");
            std::fs::copy(source, target.join("manifest.toml")).expect("copy");
        }

        let rogue = root.join("evil");
        std::fs::create_dir_all(&rogue).expect("create");
        std::fs::write(
            rogue.join("manifest.toml"),
            "id = \"evil\"\ndisplay_name = \"Evil\"\n",
        )
        .expect("write");

        let registry = TemplateRegistry::load(root).expect("load");
        assert!(
            registry.get("evil").is_none(),
            "a rogue template was loaded"
        );
        assert_eq!(registry.ids().len(), 3);
    }

    #[test]
    fn a_manifest_declaring_the_wrong_id_is_refused() {
        let directory = tempfile::tempdir().expect("temp dir");
        let target = directory.path().join("nodejs");
        std::fs::create_dir_all(&target).expect("create");
        std::fs::write(
            target.join("manifest.toml"),
            "id = \"python\"\ndisplay_name = \"x\"\ndescription = \"x\"\n\
             package_managers = []\ndefault_package_manager = \"NONE\"\n\
             start_forms = []\nworking_dir = \"/app\"\ndefault_port = 3000\n\
             [health_check]\nsupported = [\"NONE\"]\ndefault = \"NONE\"\n\
             [[versions]]\nversion = \"1\"\nbase_image = \"x:1.0\"\ndefault = true\n",
        )
        .expect("write");

        assert!(matches!(
            TemplateRegistry::load(directory.path()),
            Err(TemplateError::Malformed { .. })
        ));
    }
}
