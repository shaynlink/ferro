//! Project-manifest support for Ferro applications.
//!
//! A project is configured by a strict, versioned `ferro.toml` file. This crate owns parsing and
//! validation so the CLI and future target tooling agree on the public project contract.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use ferro_sdk::{ApplicationDescriptor, Target};
use serde::Deserialize;

/// The only manifest schema accepted by this version of Ferro.
pub const CURRENT_SCHEMA: u32 = 2;

/// Validated configuration for a Ferro application project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectManifest {
    schema: u32,
    application: ApplicationDescriptor,
    targets: Vec<Target>,
    macos: MacosConfiguration,
    version: Option<String>,
    build_number: Option<u64>,
}

/// macOS-specific project configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MacosConfiguration {
    package: Option<String>,
    signing_identity: Option<String>,
    icon: Option<PathBuf>,
    resources_dir: Option<PathBuf>,
}

impl MacosConfiguration {
    /// Returns the optional Cargo package name overriding `<slug>-macos`.
    #[must_use]
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    /// Returns the optional code-signing identity used for local macOS bundles.
    #[must_use]
    pub fn signing_identity(&self) -> Option<&str> {
        self.signing_identity.as_deref()
    }

    /// Returns the optional project-relative ICNS icon path.
    #[must_use]
    pub fn icon(&self) -> Option<&Path> {
        self.icon.as_deref()
    }

    /// Returns the optional project-relative static-resource directory.
    #[must_use]
    pub fn resources_dir(&self) -> Option<&Path> {
        self.resources_dir.as_deref()
    }
}

impl ProjectManifest {
    /// Validates and creates a project manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the name, identifier, or target list is invalid.
    pub fn new(
        name: impl Into<String>,
        id: impl Into<String>,
        targets: Vec<Target>,
    ) -> Result<Self, ProjectError> {
        let name = name.into();
        validate_name(&name)?;
        let id = id.into();
        validate_application_id(&id)?;
        validate_targets(&targets)?;
        Ok(Self {
            schema: CURRENT_SCHEMA,
            application: ApplicationDescriptor::new(name, id),
            targets,
            macos: MacosConfiguration::default(),
            version: Some("0.1.0".to_owned()),
            build_number: Some(1),
        })
    }

    /// Sets the optional Cargo package name used for the macOS host binary.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the package name is empty or contains whitespace.
    pub fn with_macos_package(mut self, package: Option<String>) -> Result<Self, ProjectError> {
        validate_package_name(package.as_deref())?;
        self.macos.package = package;
        Ok(self)
    }

    /// Sets the optional macOS code-signing identity.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the identity is empty.
    pub fn with_macos_signing_identity(
        mut self,
        signing_identity: Option<String>,
    ) -> Result<Self, ProjectError> {
        validate_signing_identity(signing_identity.as_deref())?;
        self.macos.signing_identity = signing_identity;
        Ok(self)
    }

    /// Sets the optional project-relative macOS packaging assets.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when an asset path is absolute or empty.
    pub fn with_macos_assets(
        mut self,
        icon: Option<PathBuf>,
        resources_dir: Option<PathBuf>,
    ) -> Result<Self, ProjectError> {
        validate_relative_asset_path(icon.as_deref(), "macOS icon")?;
        validate_relative_asset_path(resources_dir.as_deref(), "macOS resources_dir")?;
        self.macos.icon = icon;
        self.macos.resources_dir = resources_dir;
        Ok(self)
    }

    /// Reads and validates `ferro.toml` at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the file cannot be read, parsed, or validated.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|error| ProjectError::Read {
            path: path.to_owned(),
            error,
        })?;
        Self::parse(&source)
    }

    /// Parses and validates a manifest document.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the document has an unsupported schema or invalid content.
    pub fn parse(source: &str) -> Result<Self, ProjectError> {
        let raw: RawManifest = toml::from_str(source).map_err(ProjectError::Parse)?;
        if raw.schema != 1 && raw.schema != CURRENT_SCHEMA {
            return Err(ProjectError::UnsupportedSchema(raw.schema));
        }
        let targets = raw
            .app
            .targets
            .into_iter()
            .map(|target| {
                target
                    .parse::<Target>()
                    .map_err(|error| ProjectError::Invalid(error.to_string()))
            })
            .collect::<Result<_, _>>()?;
        let macos = raw.macos.unwrap_or_default();
        validate_package_name(macos.package.as_deref())?;
        validate_signing_identity(macos.signing_identity.as_deref())?;
        if raw.schema == 1 {
            return Ok(Self {
                schema: 1,
                application: ApplicationDescriptor::new(raw.app.name, raw.app.id),
                targets,
                macos: MacosConfiguration {
                    package: macos.package,
                    signing_identity: macos.signing_identity,
                    icon: None,
                    resources_dir: None,
                },
                version: None,
                build_number: None,
            });
        }
        let version = raw
            .app
            .version
            .ok_or_else(|| ProjectError::Invalid("schema 2 requires app.version".to_owned()))?;
        validate_version(&version)?;
        let build_number = raw.app.build_number.ok_or_else(|| {
            ProjectError::Invalid("schema 2 requires app.build_number".to_owned())
        })?;
        if build_number == 0 {
            return Err(ProjectError::Invalid(
                "app.build_number must be greater than zero".to_owned(),
            ));
        }
        Self::new(raw.app.name, raw.app.id, targets)?
            .with_macos_package(macos.package)?
            .with_macos_signing_identity(macos.signing_identity)?
            .with_macos_assets(
                macos.icon.map(PathBuf::from),
                macos.resources_dir.map(PathBuf::from),
            )
            .map(|mut manifest| {
                manifest.version = Some(version);
                manifest.build_number = Some(build_number);
                manifest
            })
    }

    /// Returns the stable application metadata.
    #[must_use]
    pub fn application(&self) -> &ApplicationDescriptor {
        &self.application
    }

    /// Returns the manifest schema version that was parsed.
    #[must_use]
    pub const fn schema(&self) -> u32 {
        self.schema
    }

    /// Returns the app-store-style marketing version when available.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Returns the monotonically increasing build number when available.
    #[must_use]
    pub const fn build_number(&self) -> Option<u64> {
        self.build_number
    }

    /// Returns enabled targets in manifest order.
    #[must_use]
    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    /// Returns macOS-specific configuration.
    #[must_use]
    pub fn macos(&self) -> &MacosConfiguration {
        &self.macos
    }

    /// Renders this manifest in Ferro's canonical TOML form.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let targets = self
            .targets
            .iter()
            .map(|target| format!("\"{target}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let mut manifest = format!(
            "schema = {}\n\n[app]\nname = \"{}\"\nid = \"{}\"\ntargets = [{targets}]\n",
            self.schema,
            self.application.name(),
            self.application.id()
        );
        if let (Some(version), Some(build_number)) = (self.version(), self.build_number()) {
            manifest.push_str("version = \"");
            manifest.push_str(version);
            manifest.push_str("\"\nbuild_number = ");
            manifest.push_str(&build_number.to_string());
            manifest.push('\n');
        }
        let mut has_macos_section = false;
        if let Some(package) = self.macos.package() {
            manifest.push_str("\n[macos]\npackage = \"");
            manifest.push_str(package);
            manifest.push_str("\"\n");
            has_macos_section = true;
        }
        if let Some(identity) = self.macos.signing_identity() {
            if !has_macos_section {
                manifest.push_str("\n[macos]\n");
                has_macos_section = true;
            }
            manifest.push_str("signing_identity = \"");
            manifest.push_str(identity);
            manifest.push_str("\"\n");
        }
        if let Some(icon) = self.macos.icon() {
            if !has_macos_section {
                manifest.push_str("\n[macos]\n");
                has_macos_section = true;
            }
            manifest.push_str("icon = \"");
            manifest.push_str(&icon.display().to_string());
            manifest.push_str("\"\n");
        }
        if let Some(resources_dir) = self.macos.resources_dir() {
            if !has_macos_section {
                manifest.push_str("\n[macos]\n");
            }
            manifest.push_str("resources_dir = \"");
            manifest.push_str(&resources_dir.display().to_string());
            manifest.push_str("\"\n");
        }
        manifest
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema: u32,
    app: RawApplication,
    macos: Option<RawMacos>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApplication {
    name: String,
    id: String,
    targets: Vec<String>,
    version: Option<String>,
    build_number: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
struct RawMacos {
    package: Option<String>,
    signing_identity: Option<String>,
    icon: Option<String>,
    resources_dir: Option<String>,
}

/// An error while loading or validating a Ferro project manifest.
#[derive(Debug)]
pub enum ProjectError {
    /// The manifest file could not be read.
    Read {
        /// Manifest path.
        path: std::path::PathBuf,
        /// Underlying filesystem error.
        error: std::io::Error,
    },
    /// The manifest is not valid TOML or has fields outside the schema.
    Parse(toml::de::Error),
    /// The manifest declares an unsupported schema number.
    UnsupportedSchema(u32),
    /// A semantic manifest invariant was violated.
    Invalid(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, error } => write!(
                formatter,
                "could not read Ferro manifest `{}`: {error}",
                path.display()
            ),
            Self::Parse(error) => write!(formatter, "invalid Ferro manifest: {error}"),
            Self::UnsupportedSchema(schema) => write!(
                formatter,
                "unsupported Ferro manifest schema {schema}; expected {CURRENT_SCHEMA}"
            ),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { error, .. } => Some(error),
            Self::Parse(error) => Some(error),
            Self::UnsupportedSchema(_) | Self::Invalid(_) => None,
        }
    }
}

fn validate_name(name: &str) -> Result<(), ProjectError> {
    if name.trim().is_empty() {
        return Err(ProjectError::Invalid(
            "application name cannot be empty".to_owned(),
        ));
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<(), ProjectError> {
    let valid = !version.is_empty()
        && version.split('.').all(|component| {
            !component.is_empty()
                && component
                    .chars()
                    .all(|character| character.is_ascii_digit())
        });
    if valid {
        Ok(())
    } else {
        Err(ProjectError::Invalid(
            "app.version must be dot-separated numeric components".to_owned(),
        ))
    }
}

fn validate_relative_asset_path(path: Option<&Path>, label: &str) -> Result<(), ProjectError> {
    let Some(path) = path else {
        return Ok(());
    };
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(ProjectError::Invalid(format!(
            "{label} must be a non-empty project-relative path"
        )));
    }
    Ok(())
}

fn validate_package_name(package: Option<&str>) -> Result<(), ProjectError> {
    let Some(package) = package else {
        return Ok(());
    };
    let valid = !package.is_empty()
        && package.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        });
    if valid {
        Ok(())
    } else {
        Err(ProjectError::Invalid(
            "macOS package must be a non-empty Cargo package identifier".to_owned(),
        ))
    }
}

fn validate_signing_identity(identity: Option<&str>) -> Result<(), ProjectError> {
    if identity.is_some_and(|identity| identity.trim().is_empty()) {
        Err(ProjectError::Invalid(
            "macOS signing_identity cannot be empty".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn validate_application_id(id: &str) -> Result<(), ProjectError> {
    let valid = id.split('.').count() >= 3
        && id.split('.').all(|component| {
            !component.is_empty()
                && component
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        });
    if !valid {
        return Err(ProjectError::Invalid(
            "application id must contain at least three dot-separated ASCII labels".to_owned(),
        ));
    }
    Ok(())
}

fn validate_targets(targets: &[Target]) -> Result<(), ProjectError> {
    if targets.is_empty() {
        return Err(ProjectError::Invalid(
            "at least one desktop target must be declared".to_owned(),
        ));
    }
    let unique = targets.iter().copied().collect::<HashSet<_>>();
    if unique.len() != targets.len() {
        return Err(ProjectError::Invalid(
            "a Ferro manifest cannot declare a target more than once".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ferro_sdk::Target;

    use super::{CURRENT_SCHEMA, ProjectManifest};

    #[test]
    fn parses_a_desktop_manifest() {
        let manifest = ProjectManifest::parse(
            "schema = 1\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"macos\", \"linux\"]\n",
        )
        .expect("valid manifest");

        assert_eq!(manifest.application().name(), "Atlas");
        assert_eq!(manifest.targets(), [Target::Macos, Target::Linux]);
        assert_eq!(manifest.schema(), 1);
        assert_eq!(manifest.version(), None);
    }

    #[test]
    fn rejects_unknown_fields() {
        let error = ProjectManifest::parse(
            "schema = 1\nextra = true\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"macos\"]\n",
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_unknown_and_duplicate_targets() {
        assert!(ProjectManifest::parse(
            "schema = 1\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"ios\"]\n",
        )
        .is_err());
        assert!(
            ProjectManifest::new(
                "Atlas",
                "dev.ferro.atlas",
                vec![Target::Macos, Target::Macos],
            )
            .is_err()
        );
    }

    #[test]
    fn renders_the_current_schema() {
        let manifest = ProjectManifest::new(
            "Atlas",
            "dev.ferro.atlas",
            vec![Target::Macos, Target::Windows, Target::Linux],
        )
        .expect("valid manifest");

        assert!(
            manifest
                .to_toml()
                .starts_with(&format!("schema = {CURRENT_SCHEMA}"))
        );
    }

    #[test]
    fn parses_and_renders_an_optional_macos_signing_identity() {
        let manifest = ProjectManifest::parse(
            "schema = 2\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"macos\"]\nversion = \"1.2.3\"\nbuild_number = 42\n\n[macos]\nsigning_identity = \"Developer ID Application: Atlas\"\nicon = \"assets/macos/AppIcon.icns\"\nresources_dir = \"assets/macos/resources\"\n",
        )
        .expect("valid signing configuration");

        assert_eq!(
            manifest.macos().signing_identity(),
            Some("Developer ID Application: Atlas")
        );
        assert!(manifest.to_toml().contains("[macos]"));
        assert_eq!(manifest.version(), Some("1.2.3"));
        assert_eq!(manifest.build_number(), Some(42));
        assert_eq!(
            manifest.macos().icon(),
            Some(Path::new("assets/macos/AppIcon.icns"))
        );
    }

    #[test]
    fn rejects_an_empty_macos_signing_identity() {
        let error = ProjectManifest::parse(
            "schema = 1\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"macos\"]\n\n[macos]\nsigning_identity = \"   \"\n",
        )
        .expect_err("empty identity must be rejected");

        assert!(
            error
                .to_string()
                .contains("signing_identity cannot be empty")
        );
    }

    #[test]
    fn rejects_incomplete_schema_two_distribution_metadata() {
        let error = ProjectManifest::parse(
            "schema = 2\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"macos\"]\n",
        )
        .expect_err("schema two requires release metadata");

        assert!(error.to_string().contains("requires app.version"));
    }
}
